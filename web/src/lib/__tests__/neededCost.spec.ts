import { describe, expect, it } from 'vitest'
import { neededCostLine, neededEntryPrice, planWishlistTopUps } from '../neededCost'
import type { NeededTotals } from '@/lib/api'

// A stand-in for the currency store's `formatUsd`: dollars as-is, unpriced as null.
const usd = (raw: string | null | undefined) => (raw == null ? null : `$${raw}`)

function totals(over: Partial<NeededTotals> = {}): NeededTotals {
  return {
    cards: 12,
    copies: 15,
    held_usd: '41.20',
    held_unpriced_cards: 0,
    cheapest_usd: '28.10',
    cheapest_unpriced_cards: 0,
    ...over,
  }
}

describe('neededCostLine', () => {
  it('names the size and both prices', () => {
    expect(neededCostLine(totals(), usd)).toBe(
      '12 cards · 15 copies · ~$41.20 at your printings · from $28.10 at cheapest printings',
    )
  })

  it('drops the clauses that would say nothing', () => {
    // One copy per card, the cheapest printing IS the held one, nothing unpriced.
    expect(neededCostLine(totals({ cards: 1, copies: 1, cheapest_usd: '41.20' }), usd)).toBe(
      '1 card · ~$41.20 at your printings',
    )
  })

  it('names no money for an unpriced list and flags a floor', () => {
    expect(
      neededCostLine(totals({ held_usd: null, cheapest_usd: null, held_unpriced_cards: 12 }), usd),
    ).toBe('12 cards · 15 copies · 12 unpriced')
    // A held total that is a floor still shows, with the caveat beside it — the larger
    // of the two unpriced counts, since either column may be the one with the gap.
    expect(
      neededCostLine(totals({ held_unpriced_cards: 2, cheapest_unpriced_cards: 1 }), usd),
    ).toBe(
      '12 cards · 15 copies · ~$41.20 at your printings · from $28.10 at cheapest printings · 2 unpriced',
    )
  })

  it('falls back to the cheapest price when the held one is unknown', () => {
    expect(neededCostLine(totals({ held_usd: null, held_unpriced_cards: 12 }), usd)).toBe(
      '12 cards · 15 copies · from $28.10 at cheapest printings · 12 unpriced',
    )
  })
})

describe('neededEntryPrice', () => {
  it('shows the held price, and the cheapest when it is lower', () => {
    expect(neededEntryPrice({ held_usd: '3.50', cheapest_usd: '1.20' }, usd)).toBe(
      '$3.50 · from $1.20',
    )
    expect(neededEntryPrice({ held_usd: '3.50', cheapest_usd: '3.50' }, usd)).toBe('$3.50')
    expect(neededEntryPrice({ held_usd: null, cheapest_usd: '1.20' }, usd)).toBe('$1.20')
    expect(neededEntryPrice({ held_usd: null, cheapest_usd: null }, usd)).toBeNull()
  })
})

describe('planWishlistTopUps', () => {
  const entry = (id: string, needed: number) =>
    ({ card: { id }, needed }) as Parameters<typeof planWishlistTopUps>[0][number]

  it('tops a card up to the copies needed, leaving foils and satisfied cards alone', () => {
    const plan = planWishlistTopUps([entry('a', 3), entry('b', 1), entry('c', 2)], {
      // Wants one regular: two more make three.
      a: { quantity: 1, foil_quantity: 0 },
      // Already wants a foil: satisfied, nothing to write.
      b: { quantity: 0, foil_quantity: 1 },
      // Not on the wish list at all.
    })
    expect(plan).toEqual([
      { id: 'a', quantity: 3, foil_quantity: 0 },
      { id: 'c', quantity: 2, foil_quantity: 0 },
    ])
  })

  it('counts foils towards the total and never lowers a count', () => {
    // Wants 1 regular + 1 foil = 2, needs 3: the regular count rises to 2, the foil stays.
    expect(planWishlistTopUps([entry('a', 3)], { a: { quantity: 1, foil_quantity: 1 } })).toEqual([
      { id: 'a', quantity: 2, foil_quantity: 1 },
    ])
    // Wants more than needed: untouched (a second click plans nothing).
    expect(planWishlistTopUps([entry('a', 1)], { a: { quantity: 4, foil_quantity: 0 } })).toEqual(
      [],
    )
  })
})
