import { onScopeDispose, watch, type Ref } from 'vue'
import { useQueryClient, type QueryClient, type QueryKey } from '@tanstack/vue-query'

// ---------- "Hold the visible grid still while a control anchored to it is open" ----------
//
// The holdings grids are recency-sorted (`updated:desc`, the default): editing a card's
// counts moves that card to the front of the list. The quick-add controls
// (OwnedCountControl / ProductCountControl) are POPOVERS ANCHORED TO A TILE IN THAT GRID,
// so any refetch that resorts or re-lays-out the grid while one is open drags the open
// panel across the screen — a finger already on its way to "Regular +" lands on "Foil +",
// the row directly below (or on a different card entirely). It reads as "my touch is off"
// because nothing about the tap was wrong; the target moved between aiming and landing.
//
// The wish list dodged this with `deferListRefetch` (its list order is frozen until the
// next navigation), but that is a per-surface trade: the collection's tile chips read the
// list's own counts, so it can't stay frozen — it just must not reflow *right now*. This
// module is that narrower contract, and it covers both surfaces:
//
//   - while at least one anchored control is open the holdings queries that decide which
//     tiles are shown (and in what order) are still marked stale, but their active refetch
//     is skipped (`refetchType: 'none'`) and the key is recorded here;
//   - a window-focus refetch of a holdings list is skipped the same way (the collection
//     leaves `refetchOnWindowFocus` on, so returning to the tab used to resort the grid
//     under an open popover — reliably, after any spell away from the app);
//   - closing the last open control replays exactly the recorded keys, so the grid settles
//     the instant the panel is gone rather than showing stale order/membership.
//
// The order-independent per-entry and batch-count keys are deliberately NOT frozen: they
// repaint tiles in place (never reflow them) and they are what keeps the open control's own
// numbers honest.
//
// State is module-level (not a Pinia store) on purpose: `invalidate` is a plain function
// over a QueryClient called from mutation callbacks and import flows, where no active pinia
// is guaranteed — and this is query-scheduling plumbing, not user-facing state. Nothing
// reads it reactively; every consumer asks at call time.

/** How many anchored holdings controls are open right now. */
let openControls = 0

/** Reflow-causing keys whose refetch was skipped while frozen, replayed on release. Keyed
 * by a serialized form so repeated writes under one open popover queue each key once. */
const skipped = new Map<string, QueryKey>()

/** Whether a holdings control anchored to a grid is currently open, i.e. whether a refetch
 * that would resort/re-lay-out that grid must be deferred. */
export function isHoldingListFrozen(): boolean {
  return openControls > 0
}

/** Mark the grid held. Returns the matching release; callers must call it exactly once. */
export function freezeHoldingLists(): () => void {
  openControls += 1
  let released = false
  return () => {
    if (released) return
    released = true
    openControls -= 1
  }
}

/** Record a refetch that was skipped because the grid is frozen, to be replayed on release.
 * A no-op when nothing is frozen (the caller refetched normally). */
export function deferHoldingRefetch(queryKey: QueryKey): void {
  if (openControls > 0) skipped.set(JSON.stringify(queryKey), queryKey)
}

/** Replay every deferred refetch once the last anchored control has closed. Only active
 * queries are refetched — a key belonging to a page the user has since left is already
 * marked stale and settles on its next mount. */
export function flushDeferredHoldingRefetches(qc: QueryClient): void {
  if (openControls > 0 || skipped.size === 0) return
  const keys = [...skipped.values()]
  skipped.clear()
  for (const queryKey of keys) void qc.refetchQueries({ queryKey, type: 'active' })
}

/** Drop-in for `refetchOnWindowFocus` / `refetchOnReconnect` on any query whose result
 * re-lays-out a holdings grid: refetch as usual, unless a control anchored to that grid is
 * open — then defer it rather than reflowing under the open panel, and replay it on close.
 *
 * Deliberately trigger-agnostic, and it belongs on EVERY grid-shaped key `invalidate`
 * freezes, not just the flat list. The two are one guarantee: freezing the write path while
 * leaving a background trigger unguarded means tabbing away and back — or a phone's
 * offline→online blip — resorts the grid anyway. (query-core reduces both triggers to
 * `isStale`, and the freeze's own `refetchType: 'none'` invalidation is precisely what makes
 * these queries stale, so they always qualify.) */
export function refetchUnlessFrozen(query: { queryKey: QueryKey }): boolean {
  if (!isHoldingListFrozen()) return true
  deferHoldingRefetch(query.queryKey)
  return false
}

/**
 * Freeze the holdings grids for as long as `active` is true — call it from anything that
 * floats over a tile: the quick-add popovers, and the detail modal (whose body carries the
 * same steppers, and which would otherwise leave the user on a grid that rearranged while it
 * was covered). Releasing replays whatever was deferred, so the grid resorts the moment the
 * panel closes rather than under the user's finger.
 */
export function useHoldingListFreeze(active: Ref<boolean>): void {
  // The mounted QueryClient is what there is to hold back. A tree without one has no
  // holdings queries to defer either, so resolve it defensively and degrade to a plain
  // no-op — this is called from the generic detail-modal shell, which must stay mountable
  // on its own rather than inheriting a vue-query dependency it doesn't otherwise have.
  let qc: QueryClient | null = null
  try {
    qc = useQueryClient()
  } catch {
    qc = null
  }
  let release: (() => void) | null = null

  function stop() {
    if (!release) return
    release()
    release = null
    if (qc) flushDeferredHoldingRefetches(qc)
  }

  watch(
    active,
    (open) => {
      if (open) release ??= freezeHoldingLists()
      else stop()
    },
    { immediate: true },
  )
  // A control unmounted while open (navigating away, the grid repainting) must not leave the
  // hold behind — it would freeze every later write for the rest of the session.
  onScopeDispose(stop)
}
