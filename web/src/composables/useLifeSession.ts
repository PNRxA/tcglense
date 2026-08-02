import { computed, ref, type Ref } from 'vue'
import type { LifeCounter, LifeEvent, LifeSeat } from '@/lib/api'
import type { LifeRotation } from '@/lib/api/life'
import {
  COMMANDER_DAMAGE,
  countersFor,
  indexCounters,
  lethalReason,
  visibleCounters,
  type LifeCounterKind,
  type SeatCounters,
} from '@/lib/lifeCounters'
import { matPlacement, resolveLayout, type SeatPlacement } from '@/lib/lifeLayout'
import { lifeLines, type LifeLine } from '@/lib/lifeSeries'
import {
  useAdjustLifeMutation,
  useFinishLifeSessionMutation,
  useLifeSessionQuery,
  useUndoLifeEventMutation,
} from '@/composables/useLifeCounter'
import { useLifeTaps } from '@/composables/useLifeTaps'
import { useWakeLock } from '@/composables/useWakeLock'

/**
 * The live life-counter engine: everything `LifeSessionView` needs to render and drive one
 * tracked game, so the view stays a template.
 *
 * It layers four things that are each tested (or testable) on their own:
 *
 * - the session read (`useLifeCounter`),
 * - the tap batching (`useLifeTaps`) — the display total is the server's plus whatever is still
 *   pending, so a tap shows instantly and the history still records one change per run of taps,
 * - the placement maths (`lib/lifeLayout`) — seats resolved to grid cells and rotations,
 * - the screen wake lock (`useWakeLock`), held only while a game is actually in progress: a
 *   finished scoreboard has no claim on your battery.
 */

/**
 * What a run of taps accumulates against: one seat's life, or one of its counters — and, for
 * commander damage, one particular opponent's commander.
 *
 * This is the client's name for the server's replay chain (`replay_seat` folds one chain per
 * `(counter, source)`), which is why batching keys by it: a 7-point commander hit becomes one
 * history row rather than seven, and it can commit alongside the life tap that follows without
 * either being applied against the other's value.
 */
export interface TapTarget {
  playerId: number
  /** Absent = the seat's life total. */
  counter?: LifeCounterKind
  /** Only ever set for `commander_damage`. */
  sourcePlayerId?: number
}

/** The chain a target belongs to, as a stable key for the tap engine. */
export function tapKey(target: TapTarget): string {
  return `${target.playerId}:${target.counter ?? 'life'}:${target.sourcePlayerId ?? ''}`
}

/** One seat, ready to render: the server's row plus where it sits and what to show right now. */
export interface LifeSeatView {
  seat: LifeSeat
  placement: SeatPlacement
  /** The total to display: committed life plus any uncommitted taps. */
  life: number
  /** Uncommitted delta, for the "−3" chip that shows before the commit lands. */
  pending: number
  /** That seat's life over time, for its sparkline. */
  line: LifeLine | undefined
  /** The seat's counters as the server derived them — never re-folded here. */
  counters: SeatCounters
  /**
   * Why this seat is out, if it is ("21 commander damage") — a **suggestion** shown on the
   * tile and offered when finishing, never something that finishes the game itself.
   */
  lethal: string | null
}

export function useLifeSession(game: Ref<string>, sessionId: Ref<number>) {
  const query = useLifeSessionQuery(game, sessionId)
  const detail = computed(() => query.data.value)
  const session = computed(() => detail.value?.session)
  const seats = computed<LifeSeat[]>(() => session.value?.players ?? [])
  const events = computed<LifeEvent[]>(() => detail.value?.events ?? [])
  const isActive = computed(() => session.value?.status === 'active')

  // The counter state, exactly as the server folded it — indexed for lookup, never re-derived.
  const counterRows = computed<LifeCounter[]>(() => detail.value?.counters ?? [])
  const countersByPlayer = computed(() => indexCounters(counterRows.value))
  /** Which counter rows the mat shows: the ones tracked, plus any that hold a value anyway. */
  const shownCounters = computed<LifeCounterKind[]>(() =>
    visibleCounters(session.value?.counters ?? [], counterRows.value),
  )

  const adjust = useAdjustLifeMutation()
  const finish = useFinishLifeSessionMutation()
  const undo = useUndoLifeEventMutation()

  const taps = useLifeTaps<TapTarget>({
    key: tapKey,
    commit: (target, delta) =>
      adjust.mutateAsync({
        game: game.value,
        sessionId: sessionId.value,
        playerId: target.playerId,
        change: {
          delta,
          ...(target.counter ? { counter: target.counter } : {}),
          ...(target.sourcePlayerId !== undefined
            ? { source_player_id: target.sourcePlayerId }
            : {}),
        },
      }),
  })

  // Only hold the screen awake for a game being played — and only once it has actually loaded,
  // so a failed read doesn't leave the lock held on an error page.
  const wakeLock = useWakeLock(() => isActive.value && seats.value.length > 0)

  const layout = computed(() => resolveLayout(session.value?.layout ?? '', seats.value.length))

  const lines = computed(() =>
    session.value ? lifeLines(seats.value, events.value, session.value.started_at) : [],
  )

  const seatViews = computed<LifeSeatView[]>(() => {
    const placement = matPlacement(
      layout.value,
      seats.value.length,
      seats.value.map((seat) => seat.rotation as LifeRotation),
    )
    const byPlayer = new Map(lines.value.map((line) => [line.playerId, line]))
    return seats.value.map((seat, index) => {
      const pending = taps.pendingFor({ playerId: seat.id })
      const counters = countersFor(countersByPlayer.value, seat.id)
      return {
        seat,
        placement: placement.seats[index] ?? { column: 'span 1', row: 'span 1', rotation: 0 },
        life: seat.life + pending,
        pending,
        line: byPlayer.get(seat.id),
        counters,
        // Read against the *displayed* life, so a seat tapped to zero reads as out before the
        // commit lands — the same immediacy the total itself has.
        lethal: lethalReason({ ...seat, life: seat.life + pending }, counters),
      }
    })
  })

  const grid = computed(() => {
    const placement = matPlacement(layout.value, seats.value.length)
    return { gridTemplateColumns: placement.columns, gridTemplateRows: placement.rows }
  })

  /** A tap on a seat's + / − zone. Ignored on a finished game — the mat is read-only then. */
  function bump(playerId: number, delta: number) {
    if (!isActive.value) return
    taps.bump({ playerId }, delta)
  }

  /**
   * A tap on one of a seat's counters. Batched exactly like a life tap: a commander hit for 7 is
   * seven taps of the same button, and it should read back as one 7-point hit.
   */
  function bumpCounter(target: TapTarget, delta: number) {
    if (!isActive.value) return
    taps.bump(target, delta)
  }

  /** The uncommitted delta for a counter chain, for the same "+3 in flight" chip life gets. */
  function pendingCounter(target: TapTarget): number {
    return taps.pendingFor(target)
  }

  /**
   * The value to show for a counter: what the server folded, plus anything still uncommitted.
   */
  function counterValue(target: TapTarget): number {
    const seat = countersFor(countersByPlayer.value, target.playerId)
    const committed =
      target.counter === COMMANDER_DAMAGE
        ? (seat.commanderDamage.get(target.sourcePlayerId ?? -1) ?? 0)
        : target.counter
          ? (seat.values[target.counter] ?? 0)
          : 0
    return committed + taps.pendingFor(target)
  }

  /**
   * Correct a seat's total outright. Any pending taps for that seat are discarded first —
   * they described a change *to* a number the user is now replacing, so committing them after
   * the correction would move it again.
   */
  async function setLife(playerId: number, life: number) {
    if (!isActive.value) return
    const target: TapTarget = { playerId }
    taps.discard(target)
    // A delta already sent for this seat is applied relative to the server's total, so letting it
    // land after the absolute correction would move the number the user just set.
    await taps.commit(target)
    await adjust.mutateAsync({
      game: game.value,
      sessionId: sessionId.value,
      playerId,
      change: { life },
    })
  }

  /**
   * Record the result. Pending taps are flushed *and awaited* first: finishing makes the session
   * immutable, so a life write still in flight would come back 409 and the last hit of the game
   * would be lost from the totals the result is read against.
   */
  async function finishGame(winnerPlayerId: number | null) {
    await taps.flush()
    await finish.mutateAsync({ game: game.value, sessionId: sessionId.value, winnerPlayerId })
  }

  async function undoEvent(eventId: number) {
    await undo.mutateAsync({ game: game.value, sessionId: sessionId.value, eventId })
  }

  /** The newest recorded change, which is the one an "Undo" button should offer. */
  const lastEvent = computed<LifeEvent | undefined>(() => events.value[events.value.length - 1])

  /** Focus mode: the mat fills the viewport and the page chrome gets out of the way. */
  const focused = ref(false)

  /** Elapsed play time, ticking while the game is live so the toolbar can show it. */
  const now = ref(Date.now())
  let ticker: ReturnType<typeof setInterval> | undefined
  const elapsed = computed(() => {
    const start = session.value ? Date.parse(session.value.started_at) : NaN
    if (Number.isNaN(start)) return 0
    const end = session.value?.finished_at ? Date.parse(session.value.finished_at) : now.value
    return Math.max(0, (Number.isNaN(end) ? now.value : end) - start)
  })
  function startTicker() {
    // 20s is fine for a minute-resolution label and costs nothing.
    ticker ??= setInterval(() => {
      now.value = Date.now()
    }, 20_000)
  }
  function stopTicker() {
    if (ticker !== undefined) {
      clearInterval(ticker)
      ticker = undefined
    }
  }

  return {
    query,
    detail,
    session,
    seats,
    events,
    isActive,
    layout,
    lines,
    seatViews,
    grid,
    lastEvent,
    focused,
    elapsed,
    startTicker,
    stopTicker,
    wakeLock,
    taps,
    shownCounters,
    countersByPlayer,
    bump,
    bumpCounter,
    pendingCounter,
    counterValue,
    setLife,
    finishGame,
    undoEvent,
    isFinishing: computed(() => finish.isPending.value),
    isUndoing: computed(() => undo.isPending.value),
  }
}
