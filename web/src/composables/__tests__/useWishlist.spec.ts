import { describe, it, expect, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { QueryClient, QueryObserver } from '@tanstack/vue-query'
import { invalidateWishlistData, invalidateWishlistProducts } from '@/composables/useWishlist'
import { invalidateCollectionData } from '@/composables/useCollection'

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

// Drive a card write's invalidation against an ACTIVE observer standing in for one of the open
// browse view's queries — either its list (key shape `[prefix, game, setCode, q, sort, page,
// related]`) or its summary (`[prefix-summary, game, setCode, related, …]`). The wish list defers
// BOTH refetches (`refetchType: 'none'`) so a per-card want edit neither resorts the
// recency-sorted tiles under the quick-add popover nor updates the summary-fed header out from
// under the frozen list total; the collection refetches both so its list-sourced count chips and
// stats update (issue #364 follow-up).
async function driveObservedWrite(
  invalidate: (qc: QueryClient, game: string, opts: { entryId?: string }) => void,
  queryKey: unknown[],
) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const queryFn = vi.fn<() => Promise<unknown>>().mockResolvedValue({ data: [], total: 0 })
  const observer = new QueryObserver(qc, { queryKey, queryFn, staleTime: Infinity })
  const unsub = observer.subscribe(() => {})
  await flushPromises()
  // The observed query has loaded once.
  expect(queryFn).toHaveBeenCalledTimes(1)
  queryFn.mockClear()

  invalidate(qc, 'mtg', { entryId: 'a' })
  await flushPromises()

  const refetched = queryFn.mock.calls.length > 0
  const invalidated = !!qc.getQueryCache().find({ queryKey })?.state.isInvalidated
  unsub()
  return { refetched, invalidated }
}

describe('wish-list card write invalidation (issue #364 follow-up)', () => {
  const wishListKey = ['wishlist', 'mtg', undefined, '', 'updated:desc', 1, false]
  const collectionListKey = ['collection', 'mtg', undefined, '', 'updated:desc', 1, false]
  // The summary keys the browse header reads (setCode/related trailers; the collection also
  // carries a bulk-threshold cents leaf). Both invalidations partial-match on `[prefix-summary,
  // game]`, so the trailers only need to be realistic, not exact.
  const wishSummaryKey = ['wishlist-summary', 'mtg', undefined, false]
  const collectionSummaryKey = ['collection-summary', 'mtg', undefined, false, 100]

  it('marks the wishlist browse list stale WITHOUT resorting it under the popover', async () => {
    const { refetched, invalidated } = await driveObservedWrite(invalidateWishlistData, wishListKey)
    // Marked stale, so the recency order refreshes on the next navigation...
    expect(invalidated).toBe(true)
    // ...but NOT actively refetched: the write's `refetchType: 'none'` keeps the open quick-add
    // popover's tiles from resorting (the heart repaints from the `wishlist-counts` overlay
    // instead). Against the pre-fix default refetch the active list refetched here — and the
    // ensuing recency resort is exactly what read as "the counter didn't update".
    expect(refetched).toBe(false)
  })

  it('collection twin: the browse list DOES refetch on a write (list-sourced chips)', async () => {
    // The collection grid's count chips read each entry's own list counts, so its list must
    // refetch to update them. This contrast guards that the deferral is wish-list-only — the
    // collection twin's refetch-on-write behaviour is unchanged.
    const { refetched } = await driveObservedWrite(invalidateCollectionData, collectionListKey)
    expect(refetched).toBe(true)
  })

  it('marks the wishlist summary stale WITHOUT refetching it (header coherence)', async () => {
    const { refetched, invalidated } = await driveObservedWrite(
      invalidateWishlistData,
      wishSummaryKey,
    )
    // Marked stale so it settles on the next navigation...
    expect(invalidated).toBe(true)
    // ...but NOT actively refetched: the summary feeds the header's value/copies while the list
    // total feeds its count/completion, so refetching the summary alone while the list stays
    // frozen would show a stale count beside fresh value/copies (F2). Deferring both keeps the
    // whole header coherent with the visible tiles. Reverting the summary deferral refetches it
    // here and this assertion fails.
    expect(refetched).toBe(false)
  })

  it('collection twin: the summary DOES refetch on a write (stats stay live)', async () => {
    // The collection isn't frozen, so its summary refetches on a write to keep its stats current.
    // This contrast guards that the summary deferral is wish-list-only.
    const { refetched } = await driveObservedWrite(invalidateCollectionData, collectionSummaryKey)
    expect(refetched).toBe(true)
  })
})
