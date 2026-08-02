import { describe, expect, it } from 'vitest'
import {
  DECK_ISSUE_STATUSES,
  deckIssueLabel,
  formatLabel,
  legalityLabel,
  MTG_FORMATS,
  normalizeFormatKey,
  statusOf,
} from '@/lib/legality'
import { makeCard } from '@/test/fixtures'

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

describe('the mirrored vocabulary', () => {
  // `api/src/handlers/decks/analysis/formats.rs` holds the same table and pins it with
  // `format_vocabulary_is_pinned`. The two lists are duplicated on purpose (a dropdown
  // shouldn't wait on a request), so each side writes its copy down: adding a format to
  // one alone fails the other's test rather than quietly disagreeing about what "tracked"
  // means. `GET /api/games/{game}/formats` publishes the server's copy.
  it('pins every tracked format, in display order', () => {
    expect(MTG_FORMATS.map((format) => format.key)).toEqual([
      'standard',
      'pioneer',
      'modern',
      'legacy',
      'vintage',
      'pauper',
      'commander',
      'oathbreaker',
      'paupercommander',
      'duel',
      'predh',
      'alchemy',
      'historic',
      'timeless',
      'gladiator',
      'brawl',
      'standardbrawl',
      'competitivebrawl',
      'penny',
      'oldschool',
      'premodern',
    ])
  })

  // The server sorts `issues` and picks a card's worst status by its `DeckIssueStatus`
  // enum's declaration order; this list is the banner's copy of that order.
  it('pins the breach severity order', () => {
    expect(DECK_ISSUE_STATUSES).toEqual([
      'banned',
      'not_legal',
      'commander_only',
      'off_colour',
      'over_limit',
      'restricted',
    ])
  })
})
