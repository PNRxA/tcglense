import { describe, expect, it, vi } from 'vitest'
import { QueryClient } from '@tanstack/vue-query'
import type { CollectionQuantities } from '@/lib/api'
import { makeProductHoldingQueries } from '@/composables/productHoldingQueries'

const counts: CollectionQuantities = { quantity: 0, foil_quantity: 0 }

function queries(invalidateAnalytics: boolean) {
  return makeProductHoldingQueries({
    prefix: invalidateAnalytics ? 'collection' : 'wishlist',
    invalidateAnalytics,
    getList: vi.fn<() => never>(),
    getListBySet: vi.fn<() => never>(),
    getEntry: vi.fn<() => Promise<CollectionQuantities>>(async () => counts),
    getSummary: vi.fn<() => never>(),
    getCounts: vi.fn<() => never>(),
    setEntry: vi.fn<() => Promise<CollectionQuantities>>(async () => counts),
  })
}

describe('product holding invalidation', () => {
  it('refreshes collection analytics after a sealed-product write', () => {
    const client = new QueryClient()
    const invalidate = vi.spyOn(client, 'invalidateQueries')
    queries(true).invalidate(client, 'mtg', { entryId: '123' })

    const keys = invalidate.mock.calls.flatMap(([filters]) =>
      typeof filters === 'object' && filters !== null ? [filters.queryKey] : [],
    )
    expect(keys).toContainEqual(['collection-value-history', 'mtg'])
    expect(keys).toContainEqual(['collection-movers', 'mtg'])
  })

  it('does not attach collection analytics to wish-list product writes', () => {
    const client = new QueryClient()
    const invalidate = vi.spyOn(client, 'invalidateQueries')
    queries(false).invalidate(client, 'mtg')

    const keys = invalidate.mock.calls.flatMap(([filters]) =>
      typeof filters === 'object' && filters !== null ? [filters.queryKey] : [],
    )
    expect(keys).not.toContainEqual(['collection-value-history', 'mtg'])
    expect(keys).not.toContainEqual(['collection-movers', 'mtg'])
  })
})
