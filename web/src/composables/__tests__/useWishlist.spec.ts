import { describe, it, expect } from 'vitest'
import { QueryClient } from '@tanstack/vue-query'
import { invalidateWishlistProducts } from '@/composables/useWishlist'

// Focused coverage for the product invalidation plumbing (issue #364) — no component
// mounting needed since `invalidateWishlistProducts` is a plain function over a
// QueryClient. `staleTime: Infinity` marks every seeded query fresh so the assertions
// are unambiguously about `isInvalidated` (set only by invalidateQueries), not about
// staleness computed from a zero default staleTime.
function isInvalidated(qc: QueryClient, queryKey: unknown[]) {
  return qc.getQueryCache().find({ queryKey })?.state.isInvalidated
}

describe('invalidateWishlistProducts', () => {
  it('invalidates the list, summary, counts, and scoped entry queries for the game', () => {
    const qc = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity } } })

    const listKey = ['wishlist-products', 'mtg', 1]
    const summaryKey = ['wishlist-products', 'mtg', 'summary']
    const countsKey = ['wishlist-product-counts', 'mtg', 'a,b']
    const entryKey = ['wishlist-product-entry', 'mtg', '100']
    const otherGameListKey = ['wishlist-products', 'other', 1]

    qc.setQueryData(listKey, { data: [], total: 0 })
    qc.setQueryData(summaryKey, { wanted_count: 0, wanted_value: 0 })
    qc.setQueryData(countsKey, { a: 1, b: 0 })
    qc.setQueryData(entryKey, { quantity: 1, foil_quantity: 0 })
    qc.setQueryData(otherGameListKey, { data: [], total: 0 })

    expect(isInvalidated(qc, listKey)).toBe(false)
    expect(isInvalidated(qc, summaryKey)).toBe(false)
    expect(isInvalidated(qc, countsKey)).toBe(false)
    expect(isInvalidated(qc, entryKey)).toBe(false)
    expect(isInvalidated(qc, otherGameListKey)).toBe(false)

    invalidateWishlistProducts(qc, 'mtg', { entryId: '100' })

    expect(isInvalidated(qc, listKey)).toBe(true)
    expect(isInvalidated(qc, summaryKey)).toBe(true)
    expect(isInvalidated(qc, countsKey)).toBe(true)
    expect(isInvalidated(qc, entryKey)).toBe(true)

    // A different game's product list is a disjoint key family and must be untouched.
    expect(isInvalidated(qc, otherGameListKey)).toBe(false)
  })

  it('invalidates every product entry for the game when no entryId is scoped', () => {
    const qc = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity } } })

    const entryKeyA = ['wishlist-product-entry', 'mtg', '100']
    const entryKeyB = ['wishlist-product-entry', 'mtg', '200']

    qc.setQueryData(entryKeyA, { quantity: 1, foil_quantity: 0 })
    qc.setQueryData(entryKeyB, { quantity: 2, foil_quantity: 0 })

    invalidateWishlistProducts(qc, 'mtg')

    expect(isInvalidated(qc, entryKeyA)).toBe(true)
    expect(isInvalidated(qc, entryKeyB)).toBe(true)
  })
})
