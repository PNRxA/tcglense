import { computed, onScopeDispose, ref, type ComputedRef, type Ref } from 'vue'
import type { ApiError } from '@/lib/api'

/**
 * Batch rapid taps into one committed change per **chain**.
 *
 * A life counter is tapped in runs: someone takes a hit for 5 and you tap `-1` five times, or
 * hold the button. Sending each tap would mean five requests and — worse — five history rows
 * reading "lost 1", when what happened was "lost 5". So taps accumulate into a per-chain pending
 * delta, the number on screen shows value + pending immediately, and after
 * {@link COMMIT_DELAY_MS} of quiet the accumulated delta is committed as a single change.
 *
 * A **chain** is whatever the caller keys by: one seat's life, or one seat's poison, or the
 * commander damage one seat has taken from one particular opponent. That is exactly the server's
 * own unit of replay (`replay_seat` folds one chain per `(counter, source)`), which is what
 * makes per-chain serialisation both necessary and sufficient — a commander-damage hit for 7 is
 * one row rather than seven, and it can commit in parallel with the life tap that follows it.
 *
 * Four details that matter:
 *
 * - **Commits are serialised per chain.** The server applies a delta relative to that chain's
 *   current value, so two overlapping commits for the same chain could interleave. Each chain
 *   has its own promise chain; other chains commit in parallel (four players tapping at once is
 *   normal).
 * - **A dispatched delta is still pending until its commit settles.** The displayed number is
 *   the committed value plus what's pending, and the committed value only moves when the
 *   response is folded in — so releasing the delta at dispatch would bounce the number back to
 *   its pre-tap value for the whole round trip. It's held until the request resolves (where the
 *   authoritative value already contains it) or fails (where dropping it *is* the snap-back).
 * - **A failed commit is not retried.** The pending delta is dropped and the number snaps back
 *   to whatever the server says, because a request that failed *in transit* may still have been
 *   applied — re-sending it could double the loss. Snapping back to server truth and reporting
 *   the failure is the honest outcome; the user can tap again.
 * - **Pending work is flushed, not abandoned**, when the page is backgrounded or the engine is
 *   disposed (leaving the counter), so a delta tapped a moment before switching apps still lands.
 */

/** How long a run of taps stays open before it's committed as one change. */
export const COMMIT_DELAY_MS = 650

export interface LifeTaps<T> {
  /** The uncommitted delta for a chain: waiting to be sent, plus sent but not yet confirmed. */
  pendingFor: (target: T) => number
  /** Whether any chain has taps still waiting to be sent. */
  hasPending: ComputedRef<boolean>
  /** Whether a commit is in flight. */
  isCommitting: ComputedRef<boolean>
  /** The last commit failure, or null. Cleared by the next successful commit. */
  error: Ref<ApiError | null>
  /** Add to a chain's pending delta and (re)start its commit timer. */
  bump: (target: T, delta: number) => void
  /**
   * Commit a chain's pending delta now, without waiting for the timer. Resolves once that
   * chain's commit queue has settled, so a caller that must not race it can await.
   */
  commit: (target: T) => Promise<void>
  /**
   * Commit every chain's pending delta now and resolve once every in-flight commit has settled —
   * what a write that closes the game (finishing it) has to await before it sends.
   */
  flush: () => Promise<void>
  /** Drop a chain's pending delta without committing (used before an absolute correction). */
  discard: (target: T) => void
}

export function useLifeTaps<T>(options: {
  /** A stable string identifying the chain a target belongs to. */
  key: (target: T) => string
  /** Send one accumulated delta for a chain. Rejecting surfaces on {@link LifeTaps.error}. */
  commit: (target: T, delta: number) => Promise<unknown>
  delayMs?: number
}): LifeTaps<T> {
  const delayMs = options.delayMs ?? COMMIT_DELAY_MS
  // Keyed by chain. The target is carried alongside the delta so the commit callback gets back
  // the thing that was tapped, rather than a key it would have to parse.
  const pending = ref<Record<string, { target: T; delta: number }>>({})
  // Deltas that have been taken out of `pending` and sent, but whose commit hasn't settled yet.
  const sent = ref<Record<string, number>>({})
  const inFlight = ref(0)
  const error = ref<ApiError | null>(null)

  const timers = new Map<string, ReturnType<typeof setTimeout>>()
  // One promise queue per chain, so a chain's deltas are applied in the order they were tapped.
  const queues = new Map<string, Promise<void>>()

  function clearTimer(key: string) {
    const timer = timers.get(key)
    if (timer !== undefined) {
      clearTimeout(timer)
      timers.delete(key)
    }
  }

  function take(key: string): number {
    const entry = pending.value[key]
    if (entry !== undefined) {
      const next = { ...pending.value }
      delete next[key]
      pending.value = next
    }
    return entry?.delta ?? 0
  }

  /** Move a chain's in-flight delta, dropping the entry when it nets to zero. */
  function trackSent(key: string, delta: number) {
    const next = { ...sent.value }
    const total = (next[key] ?? 0) + delta
    if (total === 0) delete next[key]
    else next[key] = total
    sent.value = next
  }

  function commitKey(key: string, target: T): Promise<void> {
    clearTimer(key)
    const delta = take(key)
    // A run of taps that nets to zero (+1 then -1) is not a change worth recording.
    if (delta === 0) return queues.get(key) ?? Promise.resolve()
    inFlight.value += 1
    // Hold the delta on screen until the server's value contains it — see the module doc.
    trackSent(key, delta)
    const previous = queues.get(key) ?? Promise.resolve()
    const next = previous
      .then(() => options.commit(target, delta))
      .then(
        () => {
          error.value = null
        },
        (cause: unknown) => {
          // Not retried on purpose — see the module doc. The number falls back to the
          // server's, which is the only value we can still trust.
          error.value = cause as ApiError
        },
      )
      .finally(() => {
        inFlight.value -= 1
        trackSent(key, -delta)
      })
    queues.set(key, next)
    return next
  }

  function commit(target: T): Promise<void> {
    return commitKey(options.key(target), target)
  }

  function bump(target: T, delta: number) {
    const key = options.key(target)
    const existing = pending.value[key]
    pending.value = { ...pending.value, [key]: { target, delta: (existing?.delta ?? 0) + delta } }
    clearTimer(key)
    timers.set(
      key,
      setTimeout(() => commitKey(key, target), delayMs),
    )
  }

  function discard(target: T) {
    const key = options.key(target)
    clearTimer(key)
    take(key)
  }

  function flush(): Promise<void> {
    for (const [key, entry] of Object.entries(pending.value)) commitKey(key, entry.target)
    // Includes chains whose commit was already in flight — a caller awaiting this needs every
    // write to have landed, not just the ones this call dispatched.
    return Promise.all(queues.values()).then(() => undefined)
  }

  function onVisibilityChange() {
    // Backgrounding is the most likely way a pending delta gets lost, so commit on the way out.
    if (document.visibilityState === 'hidden') void flush()
  }

  if (typeof document !== 'undefined') {
    document.addEventListener('visibilitychange', onVisibilityChange)
    onScopeDispose(() => document.removeEventListener('visibilitychange', onVisibilityChange))
  }

  onScopeDispose(() => {
    for (const timer of timers.values()) clearTimeout(timer)
    timers.clear()
    void flush()
  })

  return {
    pendingFor: (target: T) => {
      const key = options.key(target)
      return (pending.value[key]?.delta ?? 0) + (sent.value[key] ?? 0)
    },
    hasPending: computed(() => Object.keys(pending.value).length > 0),
    isCommitting: computed(() => inFlight.value > 0),
    error,
    bump,
    commit,
    flush,
    discard,
  }
}
