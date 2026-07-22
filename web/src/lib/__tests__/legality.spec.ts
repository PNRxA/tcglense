import { describe, expect, it } from 'vitest'
import type { DeckCardEntry } from '@/lib/api'
import {
  evaluateDeckLegality,
  formatLabel,
  legalityLabel,
  MTG_FORMATS,
  normalizeFormatKey,
  statusOf,
} from '@/lib/legality'
import { makeCard } from '@/test/fixtures'

function entry(
  id: string,
  name: string,
  legalities: Record<string, string> | null,
  over: Partial<Omit<DeckCardEntry, 'card'>> = {},
): DeckCardEntry {
  return {
    card: makeCard(id, { name, legalities }),
    section_id: 1,
    quantity: 1,
    foil_quantity: 0,
    ...over,
  }
}

describe('normalizeFormatKey', () => {
  it('accepts every curated key and label', () => {
    for (const format of MTG_FORMATS) {
      expect(normalizeFormatKey(format.key)).toBe(format.key)
      expect(normalizeFormatKey(format.label)).toBe(format.key)
    }
  })

  it('accepts community aliases, case- and punctuation-insensitively', () => {
    expect(normalizeFormatKey('EDH')).toBe('commander')
    expect(normalizeFormatKey('cEDH')).toBe('commander')
    expect(normalizeFormatKey('  E.D.H. ')).toBe('commander')
    expect(normalizeFormatKey('Pauper EDH')).toBe('paupercommander')
    expect(normalizeFormatKey('PDH')).toBe('paupercommander')
    expect(normalizeFormatKey('Comp. Brawl')).toBe('competitivebrawl')
    expect(normalizeFormatKey('Competitive Brawl')).toBe('competitivebrawl')
    expect(normalizeFormatKey('Historic Brawl')).toBe('brawl')
    expect(normalizeFormatKey('Duel Commander')).toBe('duel')
    expect(normalizeFormatKey('Standard Brawl')).toBe('standardbrawl')
    expect(normalizeFormatKey('Penny')).toBe('penny')
    expect(normalizeFormatKey('Old School')).toBe('oldschool')
    expect(normalizeFormatKey('PreDH')).toBe('predh')
  })

  it('returns null for casual/custom/blank formats (meaning "do not evaluate")', () => {
    expect(normalizeFormatKey('Cube')).toBeNull()
    expect(normalizeFormatKey('Limited')).toBeNull()
    expect(normalizeFormatKey('Casual')).toBeNull()
    expect(normalizeFormatKey('kitchen table')).toBeNull()
    expect(normalizeFormatKey('')).toBeNull()
    expect(normalizeFormatKey(null)).toBeNull()
    expect(normalizeFormatKey(undefined)).toBeNull()
  })
})

describe('popular formats', () => {
  it('marks exactly the widely-played set the card panel shows by default', () => {
    const popular = MTG_FORMATS.filter((format) => format.popular).map((format) => format.key)
    expect(popular).toEqual([
      'standard',
      'pioneer',
      'modern',
      'legacy',
      'vintage',
      'pauper',
      'commander',
      'historic',
      'timeless',
      'brawl',
    ])
  })
})

describe('formatLabel / legalityLabel', () => {
  it('labels known keys and falls back to the key itself', () => {
    expect(formatLabel('paupercommander')).toBe('Pauper Commander')
    expect(formatLabel('tlr')).toBe('tlr')
  })

  it('humanizes every status', () => {
    expect(legalityLabel('legal')).toBe('Legal')
    expect(legalityLabel('not_legal')).toBe('Not Legal')
    expect(legalityLabel('banned')).toBe('Banned')
    expect(legalityLabel('restricted')).toBe('Restricted')
  })
})

describe('statusOf', () => {
  it('reads a known status and treats anything else as unknown', () => {
    const card = makeCard('c1', { legalities: { modern: 'banned', legacy: 'weird' } })
    expect(statusOf(card, 'modern')).toBe('banned')
    expect(statusOf(card, 'legacy')).toBeNull()
    expect(statusOf(card, 'vintage')).toBeNull()
    expect(statusOf(makeCard('c2'), 'modern')).toBeNull()
  })
})

describe('evaluateDeckLegality', () => {
  const LEGAL = { commander: 'legal', vintage: 'legal' }

  it('returns null when the format is absent or not legality-tracked', () => {
    expect(evaluateDeckLegality(null, [entry('a', 'A', LEGAL)])).toBeNull()
    expect(evaluateDeckLegality('', [entry('a', 'A', LEGAL)])).toBeNull()
    expect(evaluateDeckLegality('Cube', [entry('a', 'A', LEGAL)])).toBeNull()
  })

  it('reports a clean deck with no issues', () => {
    const result = evaluateDeckLegality('Commander', [entry('a', 'A', LEGAL)])
    expect(result).not.toBeNull()
    expect(result!.formatKey).toBe('commander')
    expect(result!.formatLabel).toBe('Commander')
    expect(result!.issues).toEqual([])
    expect(result!.statusByCardId.size).toBe(0)
    expect(result!.unknownCount).toBe(0)
  })

  it('normalizes the stored format string before evaluating', () => {
    const result = evaluateDeckLegality('EDH', [entry('a', 'A', { commander: 'banned' })])
    expect(result!.issues).toHaveLength(1)
    expect(result!.issues[0]!.status).toBe('banned')
  })

  it('flags banned and not_legal, sorted by severity then name', () => {
    const result = evaluateDeckLegality('Commander', [
      entry('n1', 'Zebra', { commander: 'not_legal' }),
      entry('b1', 'Beta', { commander: 'banned' }),
      entry('b2', 'Alpha', { commander: 'banned' }),
      entry('ok', 'Fine', LEGAL),
    ])
    expect(result!.issues.map((issue) => `${issue.status}:${issue.name}`)).toEqual([
      'banned:Alpha',
      'banned:Beta',
      'not_legal:Zebra',
    ])
    expect(result!.statusByCardId.get('n1')).toBe('not_legal')
    expect(result!.statusByCardId.get('b1')).toBe('banned')
    expect(result!.statusByCardId.has('ok')).toBe(false)
  })

  it('allows a single restricted copy but flags more, counting foils', () => {
    const RESTRICTED = { vintage: 'restricted' }
    const one = evaluateDeckLegality('Vintage', [entry('r1', 'Ancestral', RESTRICTED)])
    expect(one!.issues).toEqual([])

    const foiled = evaluateDeckLegality('Vintage', [
      entry('r1', 'Ancestral', RESTRICTED, { quantity: 1, foil_quantity: 1 }),
    ])
    expect(foiled!.issues).toHaveLength(1)
    expect(foiled!.issues[0]).toMatchObject({ status: 'restricted', quantity: 2 })
  })

  it('folds copies of the same name across printings and sections', () => {
    const RESTRICTED = { vintage: 'restricted' }
    const result = evaluateDeckLegality('Vintage', [
      entry('print-a', 'Ancestral', RESTRICTED, { section_id: 1 }),
      entry('print-b', 'Ancestral', RESTRICTED, { section_id: 2 }),
    ])
    // One issue per name, but every offending printing gets a chip.
    expect(result!.issues).toHaveLength(1)
    expect(result!.issues[0]).toMatchObject({ name: 'Ancestral', quantity: 2 })
    expect(result!.statusByCardId.get('print-a')).toBe('restricted')
    expect(result!.statusByCardId.get('print-b')).toBe('restricted')
  })

  it('folds duplicate banned printings into one issue', () => {
    const result = evaluateDeckLegality('Commander', [
      entry('print-a', 'Hullbreacher', { commander: 'banned' }),
      entry('print-b', 'Hullbreacher', { commander: 'banned' }),
    ])
    expect(result!.issues).toHaveLength(1)
    expect(result!.statusByCardId.size).toBe(2)
  })

  it('never flags cards with missing or unexpected legality data', () => {
    const result = evaluateDeckLegality('Commander', [
      entry('none', 'No Data', null),
      entry('gap', 'Missing Key', { modern: 'legal' }),
      entry('odd', 'Odd Value', { commander: 'suspended' }),
    ])
    expect(result!.issues).toEqual([])
    expect(result!.unknownCount).toBe(3)
  })
})
