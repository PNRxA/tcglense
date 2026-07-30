import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope } from 'vue'
import { COMMIT_DELAY_MS, useLifeTaps } from '@/composables/useLifeTaps'

// The tap engine is the one piece of the life counter with real timing behaviour, so it's tested
// directly: batching a run of taps into one commit, serialising a seat's commits so two deltas
// can't interleave server-side, and NOT retrying a failed commit (which could double-apply).

/** Drain the microtask queue: a commit rides its seat's promise chain, so it is dispatched on
 * a microtask rather than synchronously. */
async function settle(): Promise<void> {
  for (let i = 0; i < 5; i += 1) await Promise.resolve()
}

/** Run `body` inside an effect scope so `onScopeDispose` fires when we stop it. */
function withScope<T>(body: () => T): { value: T; stop: () => void } {
  const scope = effectScope()
  const value = scope.run(body) as T
  return { value, stop: () => scope.stop() }
}

let commits: { playerId: number; delta: number }[]

beforeEach(() => {
  vi.useFakeTimers()
  commits = []
})

afterEach(() => {
  vi.useRealTimers()
})

function harness(commit?: (playerId: number, delta: number) => Promise<unknown>) {
  return withScope(() =>
    useLifeTaps({
      commit: (playerId, delta) => {
        commits.push({ playerId, delta })
        return commit ? commit(playerId, delta) : Promise.resolve()
      },
    }),
  )
}

describe('useLifeTaps', () => {
  it('shows a pending delta immediately and commits the run as one change', async () => {
    const { value: taps, stop } = harness()

    taps.bump(1, -1)
    taps.bump(1, -1)
    taps.bump(1, -1)
    // The total on screen moves at once, before anything is sent.
    expect(taps.pendingFor(1)).toBe(-3)
    expect(taps.hasPending.value).toBe(true)
    expect(commits).toEqual([])

    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    // One request, one history row: "lost 3", not three rows of "lost 1".
    expect(commits).toEqual([{ playerId: 1, delta: -3 }])
    expect(taps.pendingFor(1)).toBe(0)
    stop()
  })

  it('restarts the window on each tap, so a slow run still commits once', async () => {
    const { value: taps, stop } = harness()

    taps.bump(1, -1)
    vi.advanceTimersByTime(COMMIT_DELAY_MS - 50)
    taps.bump(1, -1)
    vi.advanceTimersByTime(COMMIT_DELAY_MS - 50)
    await settle()
    expect(commits).toEqual([])
    vi.advanceTimersByTime(50)
    await settle()
    expect(commits).toEqual([{ playerId: 1, delta: -2 }])
    stop()
  })

  it('batches each seat separately — a whole pod taps at once', async () => {
    const { value: taps, stop } = harness()

    taps.bump(1, -2)
    taps.bump(2, 3)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    expect(commits).toEqual([
      { playerId: 1, delta: -2 },
      { playerId: 2, delta: 3 },
    ])
    stop()
  })

  it('never sends a run that nets to zero', async () => {
    const { value: taps, stop } = harness()

    taps.bump(1, 1)
    taps.bump(1, -1)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    // A mis-tap corrected before it committed is not a change that happened.
    expect(commits).toEqual([])
    stop()
  })

  it('serialises a seat’s commits so the server applies the deltas in order', async () => {
    // The server applies a delta relative to the seat's current total, so a second commit must
    // not start before the first resolves.
    const resolvers: (() => void)[] = []
    const { value: taps, stop } = harness(
      () => new Promise<void>((resolve) => resolvers.push(resolve)),
    )

    taps.bump(1, -1)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    expect(commits).toHaveLength(1)

    taps.bump(1, -5)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    // Still one in flight: the second is queued behind it, not racing it.
    expect(commits).toHaveLength(1)
    expect(taps.isCommitting.value).toBe(true)

    resolvers[0]?.()
    await settle()
    expect(commits).toEqual([
      { playerId: 1, delta: -1 },
      { playerId: 1, delta: -5 },
    ])
    resolvers[1]?.()
    stop()
  })

  it('keeps the tapped delta on screen while its commit is in flight', async () => {
    // The displayed total is the seat's committed life plus what's pending, and the committed
    // life only moves when the response lands — so releasing the delta at dispatch would bounce
    // the number back to its pre-tap value for the whole round trip.
    const resolvers: (() => void)[] = []
    const { value: taps, stop } = harness(
      () => new Promise<void>((resolve) => resolvers.push(resolve)),
    )

    taps.bump(1, -3)
    expect(taps.pendingFor(1)).toBe(-3)

    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    expect(commits).toEqual([{ playerId: 1, delta: -3 }])
    // Sent, not yet confirmed: still counted, so the tile keeps showing the total the player
    // tapped their way to.
    expect(taps.pendingFor(1)).toBe(-3)
    expect(taps.isCommitting.value).toBe(true)

    resolvers[0]?.()
    await settle()
    // Now the server's own total carries it, so holding it here too would double-count.
    expect(taps.pendingFor(1)).toBe(0)
    stop()
  })

  it('drops the in-flight delta when the commit fails, snapping back to server truth', async () => {
    const { value: taps, stop } = harness(() => Promise.reject(new Error('offline')))

    taps.bump(1, -3)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    expect(taps.pendingFor(1)).toBe(0)
    stop()
  })

  it('resolves flush only once every commit has settled', async () => {
    // What finishing a game awaits: the session becomes immutable, so a life write still in
    // flight would come back 409 and the last hit would be lost from the recorded totals.
    const resolvers: (() => void)[] = []
    const { value: taps, stop } = harness(
      () => new Promise<void>((resolve) => resolvers.push(resolve)),
    )

    taps.bump(1, -1)
    taps.bump(2, -2)
    let settled = false
    const flushed = taps.flush().then(() => {
      settled = true
    })
    await settle()
    expect(commits).toHaveLength(2)
    expect(settled).toBe(false)

    resolvers.forEach((resolve) => resolve())
    await flushed
    expect(settled).toBe(true)
    stop()
  })

  it('flush also waits for a commit that was already in flight', async () => {
    const resolvers: (() => void)[] = []
    const { value: taps, stop } = harness(
      () => new Promise<void>((resolve) => resolvers.push(resolve)),
    )

    // Dispatched by the timer, so it is in flight rather than pending when flush is called.
    taps.bump(1, -1)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    expect(taps.isCommitting.value).toBe(true)

    let settled = false
    const flushed = taps.flush().then(() => {
      settled = true
    })
    await settle()
    expect(settled).toBe(false)

    resolvers.forEach((resolve) => resolve())
    await flushed
    expect(settled).toBe(true)
    stop()
  })

  it('reports a failure and does not retry it', async () => {
    const failure = new Error('offline')
    const { value: taps, stop } = harness(() => Promise.reject(failure))

    taps.bump(1, -4)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()

    expect(commits).toHaveLength(1)
    expect(taps.error.value).toBe(failure)
    // The pending delta is gone rather than queued for another attempt: a request that failed
    // in transit may still have been applied, so re-sending it could double the loss.
    expect(taps.pendingFor(1)).toBe(0)

    vi.advanceTimersByTime(COMMIT_DELAY_MS * 5)
    await settle()
    expect(commits).toHaveLength(1)
    stop()
  })

  it('clears a stale error once a commit succeeds', async () => {
    let fail = true
    const { value: taps, stop } = harness(() =>
      fail ? Promise.reject(new Error('offline')) : Promise.resolve(),
    )

    taps.bump(1, -1)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    expect(taps.error.value).not.toBeNull()

    fail = false
    taps.bump(1, -1)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    expect(taps.error.value).toBeNull()
    stop()
  })

  it('discards a seat’s pending taps without sending them', async () => {
    // Used before an absolute correction: the taps described a change to a number that is
    // about to be replaced, so committing them afterwards would move the correction.
    const { value: taps, stop } = harness()

    taps.bump(1, -3)
    taps.discard(1)
    expect(taps.pendingFor(1)).toBe(0)
    vi.advanceTimersByTime(COMMIT_DELAY_MS * 2)
    await settle()
    expect(commits).toEqual([])
    stop()
  })

  it('flushes every seat on demand', async () => {
    const { value: taps, stop } = harness()

    taps.bump(1, -1)
    taps.bump(2, -2)
    taps.flush()
    await settle()
    expect(commits).toEqual([
      { playerId: 1, delta: -1 },
      { playerId: 2, delta: -2 },
    ])
    stop()
  })

  it('flushes pending work when the engine is disposed instead of losing it', async () => {
    const { value: taps, stop } = harness()

    taps.bump(1, -7)
    // Leaving the counter (or navigating away) must not swallow a tap made a moment earlier.
    stop()
    await settle()
    expect(commits).toEqual([{ playerId: 1, delta: -7 }])
  })
})
