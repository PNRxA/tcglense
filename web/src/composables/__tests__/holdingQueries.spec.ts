import { describe, expect, it, vi } from 'vitest'
import { QueryClient } from '@tanstack/vue-query'
import type { CollectionQuantities } from '@/lib/api'
import { makeHoldingQueries } from '@/composables/holdingQueries'

const counts: CollectionQuantities = { quantity: 0, foil_quantity: 0 }

function queries(prefix: 'collection' | 'wishlist') {
  return makeHoldingQueries({
    prefix,
    countsKey: prefix === 'collection' ? 'collection-owned' : 'wishlist-counts',
    getList: vi.fn<() => never>(),
    getSetDrops: vi.fn<() => never>(),
    getSetSubtypes: vi.fn<() => never>(),
    getSummary: vi.fn<() => never>(),
    getSets: vi.fn<() => never>(),
    getBreakdown: vi.fn<() => never>(),
    getEntry: vi.fn<() => Promise<CollectionQuantities>>(async () => counts),
    getCounts: vi.fn<() => never>(),
    setEntry: vi.fn<() => Promise<CollectionQuantities>>(async () => counts),
    withBulkThreshold: prefix === 'collection',
    invalidateValueHistory: prefix === 'collection',
    deferListRefetch: prefix === 'wishlist',
  })
}

function invalidatedKeys(prefix: 'collection' | 'wishlist') {
  const client = new QueryClient()
  const invalidate = vi.spyOn(client, 'invalidateQueries')
  queries(prefix).invalidate(client, 'mtg', { entryId: '123' })
  return invalidate.mock.calls.flatMap(([filters]) =>
    typeof filters === 'object' && filters !== null ? [filters.queryKey] : [],
  )
}

describe('holding invalidation', () => {
  it('refreshes the breakdown after a write on either surface', () => {
    // The breakdown panel (issue #680) slices the summary beside it, so a per-card edit
    // must refetch both — on the wish list too, whose breakdown rides its own server cache.
    expect(invalidatedKeys('collection')).toContainEqual(['collection-breakdown', 'mtg'])
    expect(invalidatedKeys('wishlist')).toContainEqual(['wishlist-breakdown', 'mtg'])
  })

  it('keeps the price-history analytics collection-only', () => {
    expect(invalidatedKeys('collection')).toContainEqual(['collection-value-history', 'mtg'])
    expect(invalidatedKeys('wishlist')).not.toContainEqual(['collection-value-history', 'mtg'])
  })
})
