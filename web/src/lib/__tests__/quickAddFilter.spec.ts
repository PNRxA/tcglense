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
