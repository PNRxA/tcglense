import { describe, it, expect, vi, afterEach } from 'vitest'
import { h, ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { onlineManager, QueryClient, QueryObserver } from '@tanstack/vue-query'
import {
  deferHoldingRefetch,
  flushDeferredHoldingRefetches,
  freezeHoldingLists,
  isHoldingListFrozen,
  refetchUnlessFrozen,
  useHoldingListFreeze,
} from '@/composables/holdingListFreeze'
import { invalidateCollectionData, invalidateCollectionProducts } from '@/composables/useCollection'
import { invalidateWishlistData } from '@/composables/useWishlist'

// The bug this guards (reported as "sometimes the touch is off — I press the regular +
// and it adds a foil"): the holdings grids are recency-sorted, so a write — or just
// returning to the tab — refetched the list and resorted the tiles WHILE the quick-add
// popover anchored to one of them was open. The panel moved with its tile, and the tap
// already on its way to "Regular +" landed on "Foil +", the row directly below.
//
// The freeze state is module-level (see the module header for why), so every test releases
// what it takes; this backstop keeps a failing assertion from leaking a hold into the rest
// of the suite.
const releases: (() => void)[] = []
function freeze() {
  const release = freezeHoldingLists()
  releases.push(release)
  return release
}
afterEach(() => {
  while (releases.length) releases.pop()?.()
  // A leaked hold would quietly freeze every later test's writes, so fail here rather than as
  // a baffling assertion three tests further on.
  if (isHoldingListFrozen()) throw new Error('a holding-list freeze leaked out of a test')
})

describe('holding list freeze', () => {
  it('counts nested holds and ignores a double release', () => {
    expect(isHoldingListFrozen()).toBe(false)

    const first = freeze()
    const second = freeze()
    expect(isHoldingListFrozen()).toBe(true)

    // Two popovers open (a grid repaint can briefly overlap them): one closing must not
    // unfreeze the other's grid.
    first()
    expect(isHoldingListFrozen()).toBe(true)
    // A release called twice must not decrement past its own hold — that would leave the
    // counter negative and silently disable the freeze for the rest of the session.
    first()
    expect(isHoldingListFrozen()).toBe(true)

    second()
    expect(isHoldingListFrozen()).toBe(false)
  })

  it('replays a deferred refetch only once nothing is frozen, and only once', async () => {
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const refetchQueries = vi.spyOn(qc, 'refetchQueries')

    const release = freeze()
    deferHoldingRefetch(['collection', 'mtg'])
    deferHoldingRefetch(['collection', 'mtg'])

    // Still frozen: nothing replays yet.
    flushDeferredHoldingRefetches(qc)
    expect(refetchQueries).not.toHaveBeenCalled()

    release()
    flushDeferredHoldingRefetches(qc)
    // The same key deferred twice under one open popover replays as one refetch.
    expect(refetchQueries).toHaveBeenCalledTimes(1)
    expect(refetchQueries).toHaveBeenCalledWith({ queryKey: ['collection', 'mtg'], type: 'active' })

    // Drained: a later close doesn't re-run an already-replayed refetch.
    flushDeferredHoldingRefetches(qc)
    expect(refetchQueries).toHaveBeenCalledTimes(1)
  })

  it('takes and releases the hold in a tree with no QueryClient', () => {
    // `useHoldingListFreeze` is called from the generic detail-modal shell, which is mounted
    // on its own in its own suite. A tree with no QueryClient has no holdings queries to
    // defer either, so the composable must degrade to a no-op instead of throwing and
    // dragging a vue-query dependency into every caller.
    const wrapper = mount({
      setup() {
        useHoldingListFreeze(ref(true))
        return () => h('div')
      },
    })
    expect(isHoldingListFrozen()).toBe(true)
    wrapper.unmount()
    // Unmounting while open still releases — a leaked hold would freeze every later write.
    expect(isHoldingListFrozen()).toBe(false)
  })

  it('defers a window-focus refetch of a holdings list while a popover is open', () => {
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const refetchQueries = vi.spyOn(qc, 'refetchQueries')
    const queryKey = ['collection', 'mtg', undefined, '', 'updated:desc', 1, false]

    // Nothing open: refocusing the tab refetches as usual.
    expect(refetchUnlessFrozen({ queryKey })).toBe(true)

    const release = freeze()
    // Coming back to the app with the quick-add popover open is the most common way the grid
    // used to resort under the panel — and exactly when a stepper is being aimed at.
    expect(refetchUnlessFrozen({ queryKey })).toBe(false)

    // Deferred, not dropped: closing the popover settles the list.
    release()
    flushDeferredHoldingRefetches(qc)
    expect(refetchQueries).toHaveBeenCalledWith({ queryKey, type: 'active' })
  })

  it('suppresses a RECONNECT refetch too, not just window focus', async () => {
    // The guard has to be trigger-agnostic. Freezing the write path while leaving
    // `refetchOnReconnect` at its default meant a phone's offline→online blip resorted the
    // grid under the open panel anyway — and query-core reduces the trigger to `isStale`,
    // which the freeze's own `refetchType: 'none'` invalidation guarantees. Driven through a
    // real observer + onlineManager rather than by calling the helper, so the wiring this
    // exercises is the one query-core actually consults.
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    // `mount()` is what subscribes the client to focusManager/onlineManager. Without it the
    // online event never reaches the query and the two "did not refetch" assertions below
    // would pass for the wrong reason — which is why the replay assertion at the end is
    // load-bearing: it only holds if the guard actually ran and recorded the key.
    qc.mount()
    const queryKey = ['collection', 'mtg', undefined, '', 'updated:desc', 1, false]
    const queryFn = vi.fn<() => Promise<unknown>>().mockResolvedValue({ data: [], total: 0 })
    const observer = new QueryObserver(qc, {
      queryKey,
      queryFn,
      staleTime: Infinity,
      refetchOnReconnect: refetchUnlessFrozen,
    })
    const unsub = observer.subscribe(() => {})
    await flushPromises()
    expect(queryFn).toHaveBeenCalledTimes(1)
    queryFn.mockClear()

    const release = freeze()
    // A write while the panel is open marks it stale without refetching…
    qc.invalidateQueries({ queryKey: ['collection', 'mtg'], refetchType: 'none' })
    await flushPromises()
    expect(queryFn).not.toHaveBeenCalled()

    // …and the network coming back must not be the thing that resorts it.
    onlineManager.setOnline(false)
    onlineManager.setOnline(true)
    await flushPromises()
    expect(queryFn).not.toHaveBeenCalled()

    // Still deferred rather than dropped.
    release()
    flushDeferredHoldingRefetches(qc)
    await flushPromises()
    expect(queryFn).toHaveBeenCalled()

    unsub()
    qc.unmount()
    onlineManager.setOnline(true)
  })
})

/** Drive a write's invalidation against an ACTIVE observer standing in for one of the open
 * browse view's queries, reporting whether it refetched and whether it was marked stale. */
async function driveObservedWrite(
  invalidate: (qc: QueryClient, game: string, opts: { entryId?: string }) => void,
  queryKey: unknown[],
) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const queryFn = vi.fn<() => Promise<unknown>>().mockResolvedValue({ data: [], total: 0 })
  const observer = new QueryObserver(qc, { queryKey, queryFn, staleTime: Infinity })
  const unsub = observer.subscribe(() => {})
  await flushPromises()
  expect(queryFn).toHaveBeenCalledTimes(1)
  queryFn.mockClear()

  invalidate(qc, 'mtg', { entryId: 'a' })
  await flushPromises()
  const refetched = queryFn.mock.calls.length > 0
  const invalidated = !!qc.getQueryCache().find({ queryKey })?.state.isInvalidated

  // Closing the popover replays whatever the freeze held back.
  while (releases.length) releases.pop()?.()
  flushDeferredHoldingRefetches(qc)
  await flushPromises()
  const refetchedOnClose = queryFn.mock.calls.length > 0

  unsub()
  return { refetched, invalidated, refetchedOnClose }
}

describe('holding writes while a quick-add popover is open', () => {
  const collectionListKey = ['collection', 'mtg', undefined, '', 'updated:desc', 1, false]
  const collectionSummaryKey = ['collection-summary', 'mtg', undefined, false, 100]
  const collectionCountsKey = ['collection-owned', 'mtg', 'a,b']
  const collectionProductListKey = ['collection-products', 'mtg', 1]
  const wishListKey = ['wishlist', 'mtg', undefined, '', 'updated:desc', 1, false]

  it('marks the collection browse list stale WITHOUT resorting it under the panel', async () => {
    freeze()
    const { refetched, invalidated, refetchedOnClose } = await driveObservedWrite(
      invalidateCollectionData,
      collectionListKey,
    )
    // Stale, so the recency order is known to be out of date...
    expect(invalidated).toBe(true)
    // ...but not resorted while the popover is anchored to one of those tiles. Without the
    // freeze this refetched here — and the reflow is what moved "Foil +" under the finger.
    expect(refetched).toBe(false)
    // Deferred, not dropped: the grid settles as soon as the panel is gone.
    expect(refetchedOnClose).toBe(true)
  })

  it('holds the summary back with it, so the header stays coherent with the tiles', async () => {
    freeze()
    const { refetched, refetchedOnClose } = await driveObservedWrite(
      invalidateCollectionData,
      collectionSummaryKey,
    )
    // The list total feeds the header's count while the summary feeds its value/copies, so
    // refetching one without the other shows fresh stats beside a stale count.
    expect(refetched).toBe(false)
    expect(refetchedOnClose).toBe(true)
  })

  it('keeps the order-independent counts live so the open panel stays authoritative', async () => {
    freeze()
    const { refetched } = await driveObservedWrite(invalidateCollectionData, collectionCountsKey)
    // The batch counts repaint tiles IN PLACE — they can never reflow the grid, and freezing
    // them would leave the open control's own chips stale.
    expect(refetched).toBe(true)
  })

  it('freezes the sealed-product grid the same way', async () => {
    freeze()
    const { invalidated, refetched, refetchedOnClose } = await driveObservedWrite(
      invalidateCollectionProducts,
      collectionProductListKey,
    )
    expect(invalidated).toBe(true)
    expect(refetched).toBe(false)
    expect(refetchedOnClose).toBe(true)
  })

  it('leaves the wish list on its hold-until-navigation contract', async () => {
    freeze()
    const { invalidated, refetched, refetchedOnClose } = await driveObservedWrite(
      invalidateWishlistData,
      wishListKey,
    )
    expect(invalidated).toBe(true)
    expect(refetched).toBe(false)
    // The wish list freezes its tile order for the whole visit (`deferListRefetch`), so
    // closing the popover must NOT resort it — its hearts repaint from the counts overlay.
    expect(refetchedOnClose).toBe(false)
  })

  it('collection writes still refetch the grid with no popover open', async () => {
    const { refetched } = await driveObservedWrite(invalidateCollectionData, collectionListKey)
    // The contrast that keeps the freeze narrow: unfrozen, the collection's list-sourced count
    // chips and stats refresh on a write exactly as before.
    expect(refetched).toBe(true)
  })
})
