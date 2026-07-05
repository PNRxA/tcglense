import { describe, it, expect } from 'vitest'
import type { Card } from '@/lib/api'
import { matchPrinting } from '../match'

function print(
  id: string,
  overrides: Partial<Card> & Pick<Card, 'set_code' | 'collector_number'>,
): Card {
  return {
    id,
    name: 'Lightning Bolt',
    set_name: 'Set',
    rarity: 'common',
    lang: 'en',
    released_at: '2024-01-01',
    mana_cost: null,
    cmc: null,
    type_line: null,
    oracle_text: null,
    power: null,
    toughness: null,
    loyalty: null,
    color_identity: [],
    colors: [],
    layout: 'normal',
    prices: { usd: null, usd_foil: null, eur: null, tix: null },
    has_image: false,
    drop_name: null,
    drop_slug: null,
    faces: [],
    ...overrides,
  }
}

// Newest-first, mirroring getCardPrintingsByName's ordering.
const prints: Card[] = [
  print('a', { set_code: 'clu', collector_number: '141' }),
  print('b', { set_code: 'neo', collector_number: '133' }),
  print('c', { set_code: 'mh2', collector_number: '0123' }),
  print('d', { set_code: 'neo', collector_number: '412' }),
]

describe('matchPrinting', () => {
  it('returns null for an empty printing list or an empty hint', () => {
    expect(matchPrinting([], { setCode: 'neo' })).toBeNull()
    expect(matchPrinting(prints, {})).toBeNull()
  })

  it('matches set code + collector number exactly, case-insensitively', () => {
    expect(matchPrinting(prints, { setCode: 'NEO', collectorNumber: '133' })?.id).toBe('b')
  })

  it('ignores zero-padding differences in the collector number', () => {
    expect(matchPrinting(prints, { setCode: 'mh2', collectorNumber: '123' })?.id).toBe('c')
  })

  it('falls back to the newest printing in a set when only the set code is known', () => {
    // 'b' precedes 'd' in the newest-first list, so it wins for set neo.
    expect(matchPrinting(prints, { setCode: 'neo' })?.id).toBe('b')
  })

  it('returns null for a collector number with no set (too ambiguous)', () => {
    expect(matchPrinting(prints, { collectorNumber: '133' })).toBeNull()
  })

  it('falls back to the newest printing in the set when the exact number is not found', () => {
    // The set code read cleanly but the number didn't — still better than ignoring the set.
    expect(matchPrinting(prints, { setCode: 'neo', collectorNumber: '999' })?.id).toBe('b')
  })

  it('returns null when the set code matches nothing (fall back to the caller default)', () => {
    expect(matchPrinting(prints, { setCode: 'zzz', collectorNumber: '133' })).toBeNull()
  })
})
