import { describe, expect, it } from 'vitest'
import type { Card, DeckCardEntry, DeckSection } from '@/lib/api'
import {
  DECK_ISSUE_STATUSES,
  deckIssueLabel,
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
  over: Partial<Omit<DeckCardEntry, 'card'>> & { card?: Partial<Card> } = {},
): DeckCardEntry {
  const { card, ...rest } = over
  return {
    card: makeCard(id, { name, legalities, ...card }),
    section_id: 1,
    quantity: 1,
    foil_quantity: 0,
    ...rest,
  }
}

const SECTIONS: DeckSection[] = [
  { id: 1, name: 'Creatures', position: 0, is_maybeboard: false },
  { id: 2, name: 'Commander', position: 1, is_maybeboard: false },
]

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
  it('marks exactly the six most-played formats — two rows of the card panel', () => {
    const popular = MTG_FORMATS.filter((format) => format.popular).map((format) => format.key)
    expect(popular).toEqual(['standard', 'pioneer', 'modern', 'legacy', 'pauper', 'commander'])
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

  it('labels every deck breach, most severe first', () => {
    expect(DECK_ISSUE_STATUSES.map(deckIssueLabel)).toEqual([
      'Banned',
      'Not Legal',
      'Commander Only',
      'Off Colour',
      'Over Limit',
      'Restricted',
    ])
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

  it('reads Pauper Commander\'s "restricted" as commander-only, not a copy limit', () => {
    const RESTRICTED = { paupercommander: 'restricted' }
    const result = evaluateDeckLegality(
      'Pauper Commander',
      [
        entry('lead', 'Uncommon Lead', RESTRICTED, { section_id: 2 }),
        entry('stray', 'Uncommon Stray', RESTRICTED, { section_id: 1 }),
      ],
      SECTIONS,
    )
    // The commander itself is exactly where an uncommon belongs; the same card in the 99
    // is not, even at a single copy.
    expect(result!.issues).toEqual([
      { cardId: 'stray', name: 'Uncommon Stray', status: 'commander_only', quantity: 1 },
    ])
    expect(result!.statusByCardId.has('lead')).toBe(false)
  })
})

describe('evaluateDeckLegality + deck-construction rules', () => {
  const LEGAL = { commander: 'legal' }
  const legendary = { type_line: 'Legendary Creature — Phyrexian Angel Horror' }

  function deck(over: Partial<Omit<DeckCardEntry, 'card'>> = {}, count = 1): DeckCardEntry[] {
    return Array.from({ length: count }, (_, index) =>
      entry(`filler-${index}`, `Filler ${index}`, LEGAL, { section_id: 1, ...over }),
    )
  }

  it('carries the construction violations alongside the card issues', () => {
    const result = evaluateDeckLegality(
      'Commander',
      [entry('atraxa', 'Atraxa', LEGAL, { section_id: 2, card: legendary }), ...deck({}, 10)],
      SECTIONS,
    )
    expect(result!.issues).toEqual([])
    expect(result!.violations).toEqual([
      { rule: 'deck-size', severity: 'warning', message: '11 of 100 cards — 89 to go.' },
    ])
    // Warnings alone don't make a deck illegal — it just isn't finished.
    expect(result!.legal).toBe(true)
  })

  it('marks the deck illegal on an error-severity violation', () => {
    const result = evaluateDeckLegality(
      'Commander',
      [
        entry('ring', 'Sol Ring', LEGAL, { section_id: 2, card: { type_line: 'Artifact' } }),
        ...deck({}, 99),
      ],
      SECTIONS,
    )
    expect(result!.legal).toBe(false)
    expect(result!.violations.map((violation) => violation.rule)).toEqual(['commander-eligibility'])
  })

  it('chips the deck-rule breaches onto the same per-card map', () => {
    const result = evaluateDeckLegality(
      'Commander',
      [
        entry('cmd', 'Commander', LEGAL, {
          section_id: 2,
          card: { ...legendary, color_identity: ['W'] },
        }),
        entry('bolt', 'Lightning Bolt', LEGAL, {
          card: { type_line: 'Instant', color_identity: ['R'] },
        }),
        entry('ring', 'Sol Ring', LEGAL, {
          card: { type_line: 'Artifact', color_identity: [] },
          quantity: 2,
        }),
      ],
      SECTIONS,
    )
    expect(result!.statusByCardId.get('bolt')).toBe('off_colour')
    expect(result!.statusByCardId.get('ring')).toBe('over_limit')
    expect(result!.issues.map((issue) => `${issue.status}:${issue.name}`)).toEqual([
      'off_colour:Lightning Bolt',
      'over_limit:Sol Ring',
    ])
    expect(result!.legal).toBe(false)
  })

  it('keeps the most severe status when two rules catch the same card', () => {
    const result = evaluateDeckLegality(
      'Commander',
      [
        entry('cmd', 'Commander', LEGAL, {
          section_id: 2,
          card: { ...legendary, color_identity: ['W'] },
        }),
        entry(
          'bolt',
          'Lightning Bolt',
          { commander: 'banned' },
          {
            card: { type_line: 'Instant', color_identity: ['R'] },
            quantity: 3,
          },
        ),
      ],
      SECTIONS,
    )
    expect(result!.statusByCardId.get('bolt')).toBe('banned')
    expect(result!.issues).toEqual([
      { cardId: 'bolt', name: 'Lightning Bolt', status: 'banned', quantity: 3 },
    ])
  })

  it('skips every deck-wide check when no sections are supplied', () => {
    const result = evaluateDeckLegality('Commander', [
      entry('bolt', 'Lightning Bolt', LEGAL, {
        card: { type_line: 'Instant', color_identity: ['R'] },
        quantity: 2,
      }),
    ])
    // Without sections there is no command zone to judge identity against, but the
    // format's own singleton rule still applies.
    expect(result!.statusByCardId.get('bolt')).toBe('over_limit')
    expect(result!.violations.map((violation) => violation.rule)).toEqual([
      'deck-size',
      'command-zone',
    ])
  })
})
