import { computed, onScopeDispose, ref, type ComputedRef, type Ref } from 'vue'
import type { ApiError } from '@/lib/api'

/**
 * Batch rapid life taps into one committed change per seat.
 *
 * A life counter is tapped in runs: someone takes a hit for 5 and you tap `-1` five times, or
 * hold the button. Sending each tap would mean five requests and — worse — five history rows
 * reading "lost 1", when what happened was "lost 5". So taps accumulate into a per-seat pending
 * delta, the total on screen shows life + pending immediately, and after {@link COMMIT_DELAY_MS}
 * of quiet the accumulated delta is committed as a single change.
 *
 * Three details that matter:
 *
 * - **Commits are serialised per seat.** The server applies a delta relative to the seat's
 *   current total, so two overlapping commits for the same seat could interleave. Each seat has
 *   its own promise chain; other seats commit in parallel (four players tapping at once is
 *   normal).
 * - **A failed commit is not retried.** The pending delta is dropped and the total snaps back to
 *   whatever the server says, because a request that failed *in transit* may still have been
 *   applied — re-sending it could double the loss. Snapping back to server truth and reporting
 *   the failure is the honest outcome; the user can tap again.
 * - **Pending work is flushed, not abandoned**, when the page is backgrounded or the engine is
 *   disposed (leaving the counter), so a delta tapped a moment before switching apps still lands.
 */

/** How long a run of taps stays open before it's committed as one change. */
export const COMMIT_DELAY_MS = 650

export interface LifeTaps {
  /** The uncommitted delta for a seat (0 when nothing is pending). */
  pendingFor: (playerId: number) => number
  /** Whether any seat has uncommitted taps. */
  hasPending: ComputedRef<boolean>
  /** Whether a commit is in flight. */
  isCommitting: ComputedRef<boolean>
  /** The last commit failure, or null. Cleared by the next successful commit. */
  error: Ref<ApiError | null>
  /** Add to a seat's pending delta and (re)start its commit timer. */
  bump: (playerId: number, delta: number) => void
  /** Commit a seat's pending delta now, without waiting for the timer. */
  commit: (playerId: number) => void
  /** Commit every seat's pending delta now. */
  flush: () => void
  /** Drop a seat's pending delta without committing (used before an absolute correction). */
  discard: (playerId: number) => void
}

export function useLifeTaps(options: {
  /** Send one accumulated delta for a seat. Rejecting surfaces on {@link LifeTaps.error}. */
  commit: (playerId: number, delta: number) => Promise<unknown>
  delayMs?: number
}): LifeTaps {
  const delayMs = options.delayMs ?? COMMIT_DELAY_MS
  const pending = ref<Record<number, number>>({})
  const inFlight = ref(0)
  const error = ref<ApiError | null>(null)

  const timers = new Map<number, ReturnType<typeof setTimeout>>()
  // One promise chain per seat, so a seat's deltas are applied in the order they were tapped.
  const chains = new Map<number, Promise<void>>()

  function clearTimer(playerId: number) {
    const timer = timers.get(playerId)
    if (timer !== undefined) {
      clearTimeout(timer)
      timers.delete(playerId)
    }
  }

  function take(playerId: number): number {
    const delta = pending.value[playerId] ?? 0
    if (delta !== 0) {
      const next = { ...pending.value }
      delete next[playerId]
      pending.value = next
    }
    return delta
  }

  function commit(playerId: number) {
    clearTimer(playerId)
    const delta = take(playerId)
    // A run of taps that nets to zero (+1 then -1) is not a change worth recording.
    if (delta === 0) return
    inFlight.value += 1
    const previous = chains.get(playerId) ?? Promise.resolve()
    const next = previous
      .then(() => options.commit(playerId, delta))
      .then(
        () => {
          error.value = null
        },
        (cause: unknown) => {
          // Not retried on purpose — see the module doc. The total falls back to the
          // server's, which is the only value we can still trust.
          error.value = cause as ApiError
        },
      )
      .finally(() => {
        inFlight.value -= 1
      })
    chains.set(playerId, next)
  }

  function bump(playerId: number, delta: number) {
    pending.value = { ...pending.value, [playerId]: (pending.value[playerId] ?? 0) + delta }
    clearTimer(playerId)
    timers.set(
      playerId,
      setTimeout(() => commit(playerId), delayMs),
    )
  }

  function discard(playerId: number) {
    clearTimer(playerId)
    take(playerId)
  }

  function flush() {
    for (const playerId of Object.keys(pending.value)) commit(Number(playerId))
  }

  function onVisibilityChange() {
    // Backgrounding is the most likely way a pending delta gets lost, so commit on the way out.
    if (document.visibilityState === 'hidden') flush()
  }

  if (typeof document !== 'undefined') {
    document.addEventListener('visibilitychange', onVisibilityChange)
    onScopeDispose(() => document.removeEventListener('visibilitychange', onVisibilityChange))
  }

  onScopeDispose(() => {
    for (const timer of timers.values()) clearTimeout(timer)
    timers.clear()
    flush()
  })

  return {
    pendingFor: (playerId: number) => pending.value[playerId] ?? 0,
    hasPending: computed(() => Object.keys(pending.value).length > 0),
    isCommitting: computed(() => inFlight.value > 0),
    error,
    bump,
    commit,
    flush,
    discard,
  }
}
