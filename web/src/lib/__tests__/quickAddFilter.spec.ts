import { describe, it, expect } from 'vitest'
import type { Card } from '../api'
import { filterPrintings } from '../quickAddFilter'

// Minimal printings differing only in the fields the filter matches on.
function print(
  id: string,
  overrides: Partial<Card> & Pick<Card, 'set_code' | 'set_name' | 'collector_number'>,
): Card {
  return {
    id,
    name: 'Aang, Airbending Master',
    rarity: 'rare',
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
    secret_lair_bonus: false,
    secret_lair_spend_incentive: false,
    faces: [],
    ...overrides,
  }
}

const cards: Card[] = [
  print('a', { set_code: 'tla', set_name: 'Avatar: The Last Airbender', collector_number: '2672' }),
  print('b', {
    set_code: 'tla',
    set_name: 'Avatar: The Last Airbender',
    collector_number: '18',
    rarity: 'mythic',
  }),
  print('c', {
    set_code: 'sld',
    set_name: 'Secret Lair Drop',
    collector_number: '2672',
    lang: 'ja',
  }),
]

const ids = (result: Card[]) => result.map((c) => c.id)

describe('filterPrintings', () => {
  it('returns everything for a blank or whitespace query', () => {
    expect(filterPrintings(cards, '')).toEqual(cards)
    expect(filterPrintings(cards, '   ')).toEqual(cards)
  })

  it('matches the set code case-insensitively', () => {
    expect(ids(filterPrintings(cards, 'TLA'))).toEqual(['a', 'b'])
    expect(ids(filterPrintings(cards, 'sld'))).toEqual(['c'])
  })

  it('matches a set-name substring', () => {
    expect(ids(filterPrintings(cards, 'airbender'))).toEqual(['a', 'b'])
  })

  it('matches a collector number with or without the # prefix', () => {
    expect(ids(filterPrintings(cards, '2672'))).toEqual(['a', 'c'])
    expect(ids(filterPrintings(cards, '#2672'))).toEqual(['a', 'c'])
  })

  it('matches rarity and language', () => {
    expect(ids(filterPrintings(cards, 'mythic'))).toEqual(['b'])
    expect(ids(filterPrintings(cards, 'ja'))).toEqual(['c'])
  })

  it('ANDs whitespace-separated tokens', () => {
    // The tla printing numbered 2672, not the sld one also numbered 2672.
    expect(ids(filterPrintings(cards, 'tla 2672'))).toEqual(['a'])
  })

  it('returns nothing when no printing matches', () => {
    expect(filterPrintings(cards, 'zzz')).toEqual([])
  })
})

describe('filterPrintings collector-number matching (#268)', () => {
  const numbered: Card[] = [
    print('one', { set_code: 'foo', set_name: 'Foo Set', collector_number: '1' }),
    print('ten', { set_code: 'foo', set_name: 'Foo Set', collector_number: '10' }),
    print('eighteen', { set_code: 'foo', set_name: 'Foo Set', collector_number: '18' }),
    print('hundred', { set_code: 'foo', set_name: 'Foo Set', collector_number: '100' }),
    print('star', { set_code: 'foo', set_name: 'Foo Set', collector_number: '★' }),
    print('suffix', { set_code: 'foo', set_name: 'Foo Set', collector_number: '18a' }),
  ]

  it('matches a bare number exactly, not as a substring', () => {
    // The whole point of #268: "1" must not drag in 10, 18 or 100.
    expect(ids(filterPrintings(numbered, '1'))).toEqual(['one'])
    expect(ids(filterPrintings(numbered, '10'))).toEqual(['ten'])
    expect(ids(filterPrintings(numbered, '100'))).toEqual(['hundred'])
  })

  it('ignores leading zeros in the query', () => {
    expect(ids(filterPrintings(numbered, '0001'))).toEqual(['one'])
    expect(ids(filterPrintings(numbered, '01'))).toEqual(['one'])
    expect(ids(filterPrintings(numbered, '#0018'))).toEqual(['eighteen'])
  })

  it('does not match a numeric token against a non-numeric collector number', () => {
    // "18" is an exact collector-number lookup, so it never matches "18a" or "★".
    expect(ids(filterPrintings(numbered, '18'))).toEqual(['eighteen'])
    // A suffixed number is found by typing the suffix (falls back to substring).
    expect(ids(filterPrintings(numbered, '18a'))).toEqual(['suffix'])
    expect(filterPrintings(numbered, '2')).toEqual([])
  })

  it('still matches a #-prefixed non-numeric collector number', () => {
    // The '#' is a collector-number prefix; dropping it must not lose suffixed/★ numbers.
    expect(ids(filterPrintings(numbered, '#18a'))).toEqual(['suffix'])
    expect(ids(filterPrintings(numbered, '#★'))).toEqual(['star'])
  })
})

describe('filterPrintings numeric set-name matching (#268)', () => {
  const sets: Card[] = [
    print('dm22', { set_code: '2x2', set_name: 'Double Masters 2022', collector_number: '77' }),
    print('cmd21', { set_code: 'c21', set_name: 'Commander 2021', collector_number: '5' }),
  ]

  it('matches a standalone number in the set name', () => {
    expect(ids(filterPrintings(sets, '2022'))).toEqual(['dm22'])
    // Combining the visible set name works: "double masters 2022" tokenizes fine.
    expect(ids(filterPrintings(sets, 'double 2022'))).toEqual(['dm22'])
  })

  it('does not treat a number as a substring of the set name', () => {
    // "20" is inside "2022" but not a standalone number, and no collector number is 20.
    expect(filterPrintings(sets, '20')).toEqual([])
    // "2" must not drag in "Commander 2021" the way "1" used to drag in "10".
    expect(filterPrintings(sets, '2')).toEqual([])
  })

  it('still matches the collector number even when a set name carries a year', () => {
    expect(ids(filterPrintings(sets, '77'))).toEqual(['dm22'])
    expect(ids(filterPrintings(sets, '5'))).toEqual(['cmd21'])
  })
})
