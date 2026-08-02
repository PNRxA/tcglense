import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope } from 'vue'
import { COMMIT_DELAY_MS, useLifeTaps } from '@/composables/useLifeTaps'
import { tapKey, type TapTarget } from '@/composables/useLifeSession'

// The tap engine is the one piece of the life counter with real timing behaviour, so it's tested
// directly: batching a run of taps into one commit, serialising a chain's commits so two deltas
// can't interleave server-side, and NOT retrying a failed commit (which could double-apply).
//
// A *chain* is whatever the caller keys by — a seat's life, its poison, or the commander damage
// it has taken from one particular opponent. The harness below keys the way `useLifeSession`
// does, so these cases exercise the real key function rather than a test-only one.

/** Drain the microtask queue: a commit rides its chain's promise queue, so it is dispatched on
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

let commits: { target: TapTarget; delta: number }[]

beforeEach(() => {
  vi.useFakeTimers()
  commits = []
})

afterEach(() => {
  vi.useRealTimers()
})

function harness(commit?: (target: TapTarget, delta: number) => Promise<unknown>) {
  return withScope(() =>
    useLifeTaps<TapTarget>({
      key: tapKey,
      commit: (target, delta) => {
        commits.push({ target, delta })
        return commit ? commit(target, delta) : Promise.resolve()
      },
    }),
  )
}

/** A seat's life chain, the common target. */
const life = (playerId: number): TapTarget => ({ playerId })
/** What a commit for a seat's life looks like in `commits`. */
const lifeCommit = (playerId: number, delta: number) => ({ target: life(playerId), delta })

describe('useLifeTaps', () => {
  it('shows a pending delta immediately and commits the run as one change', async () => {
    const { value: taps, stop } = harness()

    taps.bump(life(1), -1)
    taps.bump(life(1), -1)
    taps.bump(life(1), -1)
    // The total on screen moves at once, before anything is sent.
    expect(taps.pendingFor(life(1))).toBe(-3)
    expect(taps.hasPending.value).toBe(true)
    expect(commits).toEqual([])

    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    // One request, one history row: "lost 3", not three rows of "lost 1".
    expect(commits).toEqual([lifeCommit(1, -3)])
    expect(taps.pendingFor(life(1))).toBe(0)
    stop()
  })

  it('restarts the window on each tap, so a slow run still commits once', async () => {
    const { value: taps, stop } = harness()

    taps.bump(life(1), -1)
    vi.advanceTimersByTime(COMMIT_DELAY_MS - 50)
    taps.bump(life(1), -1)
    vi.advanceTimersByTime(COMMIT_DELAY_MS - 50)
    await settle()
    expect(commits).toEqual([])
    vi.advanceTimersByTime(50)
    await settle()
    expect(commits).toEqual([lifeCommit(1, -2)])
    stop()
  })

  it('batches each seat separately — a whole pod taps at once', async () => {
    const { value: taps, stop } = harness()

    taps.bump(life(1), -2)
    taps.bump(life(2), 3)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    expect(commits).toEqual([lifeCommit(1, -2), lifeCommit(2, 3)])
    stop()
  })

  it('batches each counter chain separately, including per damage source', async () => {
    // A commander hit for 7 is seven taps of one button and should read back as one 7-point hit,
    // and damage from two different commanders must not be added together — the chain key is
    // what makes both true.
    const { value: taps, stop } = harness()

    const fromBob: TapTarget = { playerId: 1, counter: 'commander_damage', sourcePlayerId: 2 }
    const fromCarol: TapTarget = { playerId: 1, counter: 'commander_damage', sourcePlayerId: 3 }
    taps.bump(fromBob, 1)
    taps.bump(fromBob, 1)
    taps.bump(fromCarol, 1)
    taps.bump(life(1), -3)
    expect(taps.pendingFor(fromBob)).toBe(2)
    expect(taps.pendingFor(fromCarol)).toBe(1)
    expect(taps.pendingFor(life(1))).toBe(-3)

    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    expect(commits).toEqual([
      { target: fromBob, delta: 2 },
      { target: fromCarol, delta: 1 },
      lifeCommit(1, -3),
    ])
    stop()
  })

  it('never sends a run that nets to zero', async () => {
    const { value: taps, stop } = harness()

    taps.bump(life(1), 1)
    taps.bump(life(1), -1)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    // A mis-tap corrected before it committed is not a change that happened.
    expect(commits).toEqual([])
    stop()
  })

  it('serialises a chain’s commits so the server applies the deltas in order', async () => {
    // The server applies a delta relative to the seat's current total, so a second commit must
    // not start before the first resolves.
    const resolvers: (() => void)[] = []
    const { value: taps, stop } = harness(
      () => new Promise<void>((resolve) => resolvers.push(resolve)),
    )

    taps.bump(life(1), -1)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    expect(commits).toHaveLength(1)

    taps.bump(life(1), -5)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    // Still one in flight: the second is queued behind it, not racing it.
    expect(commits).toHaveLength(1)
    expect(taps.isCommitting.value).toBe(true)

    resolvers[0]?.()
    await settle()
    expect(commits).toEqual([lifeCommit(1, -1), lifeCommit(1, -5)])
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

    taps.bump(life(1), -3)
    expect(taps.pendingFor(life(1))).toBe(-3)

    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    expect(commits).toEqual([lifeCommit(1, -3)])
    // Sent, not yet confirmed: still counted, so the tile keeps showing the total the player
    // tapped their way to.
    expect(taps.pendingFor(life(1))).toBe(-3)
    expect(taps.isCommitting.value).toBe(true)

    resolvers[0]?.()
    await settle()
    // Now the server's own total carries it, so holding it here too would double-count.
    expect(taps.pendingFor(life(1))).toBe(0)
    stop()
  })

  it('drops the in-flight delta when the commit fails, snapping back to server truth', async () => {
    const { value: taps, stop } = harness(() => Promise.reject(new Error('offline')))

    taps.bump(life(1), -3)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    expect(taps.pendingFor(life(1))).toBe(0)
    stop()
  })

  it('resolves flush only once every commit has settled', async () => {
    // What finishing a game awaits: the session becomes immutable, so a life write still in
    // flight would come back 409 and the last hit would be lost from the recorded totals.
    const resolvers: (() => void)[] = []
    const { value: taps, stop } = harness(
      () => new Promise<void>((resolve) => resolvers.push(resolve)),
    )

    taps.bump(life(1), -1)
    taps.bump(life(2), -2)
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
    taps.bump(life(1), -1)
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

    taps.bump(life(1), -4)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()

    expect(commits).toHaveLength(1)
    expect(taps.error.value).toBe(failure)
    // The pending delta is gone rather than queued for another attempt: a request that failed
    // in transit may still have been applied, so re-sending it could double the loss.
    expect(taps.pendingFor(life(1))).toBe(0)

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

    taps.bump(life(1), -1)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    expect(taps.error.value).not.toBeNull()

    fail = false
    taps.bump(life(1), -1)
    vi.advanceTimersByTime(COMMIT_DELAY_MS)
    await settle()
    expect(taps.error.value).toBeNull()
    stop()
  })

  it('discards a chain’s pending taps without sending them', async () => {
    // Used before an absolute correction: the taps described a change to a number that is
    // about to be replaced, so committing them afterwards would move the correction.
    const { value: taps, stop } = harness()

    taps.bump(life(1), -3)
    taps.discard(life(1))
    expect(taps.pendingFor(life(1))).toBe(0)
    vi.advanceTimersByTime(COMMIT_DELAY_MS * 2)
    await settle()
    expect(commits).toEqual([])
    stop()
  })

  it('flushes every seat on demand', async () => {
    const { value: taps, stop } = harness()

    taps.bump(life(1), -1)
    taps.bump(life(2), -2)
    taps.flush()
    await settle()
    expect(commits).toEqual([lifeCommit(1, -1), lifeCommit(2, -2)])
    stop()
  })

  it('flushes pending work when the engine is disposed instead of losing it', async () => {
    const { value: taps, stop } = harness()

    taps.bump(life(1), -7)
    // Leaving the counter (or navigating away) must not swallow a tap made a moment earlier.
    stop()
    await settle()
    expect(commits).toEqual([lifeCommit(1, -7)])
  })
})
