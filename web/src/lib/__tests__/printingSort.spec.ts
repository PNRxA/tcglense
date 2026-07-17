import { describe, expect, it } from 'vitest'
import { PRINTING_DEFAULT_SORT, sortPrintings } from '@/lib/printingSort'
import { makeCard } from '@/test/fixtures'

const ids = (cards: { id: string }[]) => cards.map((c) => c.id)

describe('sortPrintings', () => {
  it('defaults to newest printing first (a no-op over the API order), non-mutating', () => {
    const input = [
      makeCard('old', { released_at: '2019-01-01' }),
      makeCard('new', { released_at: '2024-06-01' }),
      makeCard('mid', { released_at: '2021-03-01' }),
    ]
    const sorted = sortPrintings(input, PRINTING_DEFAULT_SORT)
    expect(ids(sorted)).toEqual(['new', 'mid', 'old'])
    // Returns a fresh array; the caller's list is untouched.
    expect(ids(input)).toEqual(['old', 'new', 'mid'])
    expect(sortPrintings(input, 'released:asc').map((c) => c.id)).toEqual(['old', 'mid', 'new'])
  })

  it('parks printings with no release date last in either direction', () => {
    const input = [
      makeCard('dated', { released_at: '2020-01-01' }),
      makeCard('undated', { released_at: null }),
      makeCard('newer', { released_at: '2023-01-01' }),
    ]
    expect(ids(sortPrintings(input, 'released:desc'))).toEqual(['newer', 'dated', 'undated'])
    expect(ids(sortPrintings(input, 'released:asc'))).toEqual(['dated', 'newer', 'undated'])
  })

  it('sorts by set code both ways', () => {
    const input = [
      makeCard('c', { set_code: 'zen' }),
      makeCard('a', { set_code: 'aer' }),
      makeCard('b', { set_code: 'mid' }),
    ]
    expect(ids(sortPrintings(input, 'set:asc'))).toEqual(['a', 'b', 'c'])
    expect(ids(sortPrintings(input, 'set:desc'))).toEqual(['c', 'b', 'a'])
  })

  it('sorts collector numbers numerically, with non-numeric numbers last', () => {
    const input = [
      makeCard('ten', { collector_number: '10' }),
      makeCard('two', { collector_number: '2' }),
      makeCard('star', { collector_number: '★' }),
      makeCard('one', { collector_number: '1a' }),
    ]
    // Numeric-aware: 1 < 2 < 10, and the non-numeric "★" sorts last.
    expect(ids(sortPrintings(input, 'number:asc'))).toEqual(['one', 'two', 'ten', 'star'])
  })

  it('ranks rarity by the canonical order, unknown/missing last', () => {
    const input = [
      makeCard('rare', { rarity: 'rare' }),
      makeCard('mythic', { rarity: 'mythic' }),
      makeCard('common', { rarity: 'common' }),
      makeCard('none', { rarity: null }),
    ]
    expect(ids(sortPrintings(input, 'rarity:desc'))).toEqual(['mythic', 'rare', 'common', 'none'])
    expect(ids(sortPrintings(input, 'rarity:asc'))).toEqual(['common', 'rare', 'mythic', 'none'])
  })

  it('sorts by USD price, falling back to foil price, unpriced last', () => {
    const input = [
      makeCard('cheap', { prices: { usd: '1.00', usd_foil: null, eur: null, tix: null } }),
      makeCard('pricey', { prices: { usd: '50.00', usd_foil: null, eur: null, tix: null } }),
      // Foil-only printing: its foil price stands in for the missing regular price.
      makeCard('foilonly', { prices: { usd: null, usd_foil: '10.00', eur: null, tix: null } }),
      makeCard('unpriced', { prices: { usd: null, usd_foil: null, eur: null, tix: null } }),
    ]
    expect(ids(sortPrintings(input, 'price:desc'))).toEqual([
      'pricey',
      'foilonly',
      'cheap',
      'unpriced',
    ])
    expect(ids(sortPrintings(input, 'price:asc'))).toEqual([
      'cheap',
      'foilonly',
      'pricey',
      'unpriced',
    ])
  })

  it('keeps ties in their incoming order (stable sort)', () => {
    const input = [
      makeCard('first', { released_at: '2022-01-01' }),
      makeCard('second', { released_at: '2022-01-01' }),
      makeCard('third', { released_at: '2022-01-01' }),
    ]
    expect(ids(sortPrintings(input, 'released:desc'))).toEqual(['first', 'second', 'third'])
  })
})
