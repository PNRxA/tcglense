import { describe, expect, it } from 'vitest'
import {
  heldFirstSortOptions,
  PRINTING_DEFAULT_SORT,
  PRINTING_HELD_FIRST_SORT,
  PRINTING_SORT_OPTIONS,
  sortPrintings,
} from '@/lib/printingSort'
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

describe('held-first printing sort', () => {
  const printings = [
    makeCard('newest', { released_at: '2024-01-01' }),
    makeCard('held-old', { released_at: '2015-01-01' }),
    makeCard('middle', { released_at: '2020-01-01' }),
    makeCard('held-new', { released_at: '2022-01-01' }),
  ]

  it('leads the sort menu with the held option, keeping the rest in order', () => {
    const options = heldFirstSortOptions('Owned first')
    expect(options[0]).toEqual({ value: PRINTING_HELD_FIRST_SORT, label: 'Owned first' })
    expect(options.slice(1)).toEqual(PRINTING_SORT_OPTIONS)
  })

  it('floats held printings above unheld ones, each group newest-first', () => {
    const ownership = {
      'held-old': { quantity: 1, foil_quantity: 0 },
      'held-new': { quantity: 0, foil_quantity: 2 },
    }
    expect(ids(sortPrintings(printings, PRINTING_HELD_FIRST_SORT, ownership))).toEqual([
      'held-new',
      'held-old',
      'newest',
      'middle',
    ])
  })

  it('treats a zero holding (and one absent from the map) as unheld', () => {
    const ownership = {
      'held-old': { quantity: 0, foil_quantity: 0 },
      'held-new': { quantity: 1, foil_quantity: 0 },
    }
    expect(ids(sortPrintings(printings, PRINTING_HELD_FIRST_SORT, ownership))).toEqual([
      'held-new',
      'newest',
      'middle',
      'held-old',
    ])
  })

  it('falls back to the newest-first default with no ownership map', () => {
    // What the picker renders while the counts are still loading: the pre-existing order,
    // never a claim that nothing is held.
    expect(ids(sortPrintings(printings, PRINTING_HELD_FIRST_SORT))).toEqual(
      ids(sortPrintings(printings, PRINTING_DEFAULT_SORT)),
    )
  })

  it('is dropped by any other sort — an explicit pick means exactly what it says', () => {
    const ownership = { 'held-old': { quantity: 1, foil_quantity: 0 } }
    // The held printing stays where its release date puts it — last.
    expect(ids(sortPrintings(printings, 'released:desc', ownership))).toEqual([
      'newest',
      'held-new',
      'middle',
      'held-old',
    ])
  })
})
