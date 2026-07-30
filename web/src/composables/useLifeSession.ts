import { computed, ref, type Ref } from 'vue'
import type { LifeEvent, LifeSeat } from '@/lib/api'
import type { LifeRotation } from '@/lib/api/life'
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
}

export function useLifeSession(game: Ref<string>, sessionId: Ref<number>) {
  const query = useLifeSessionQuery(game, sessionId)
  const detail = computed(() => query.data.value)
  const session = computed(() => detail.value?.session)
  const seats = computed<LifeSeat[]>(() => session.value?.players ?? [])
  const events = computed<LifeEvent[]>(() => detail.value?.events ?? [])
  const isActive = computed(() => session.value?.status === 'active')

  const adjust = useAdjustLifeMutation()
  const finish = useFinishLifeSessionMutation()
  const undo = useUndoLifeEventMutation()

  const taps = useLifeTaps({
    commit: (playerId, delta) =>
      adjust.mutateAsync({
        game: game.value,
        sessionId: sessionId.value,
        playerId,
        change: { delta },
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
      const pending = taps.pendingFor(seat.id)
      return {
        seat,
        placement: placement.seats[index] ?? { column: 'span 1', row: 'span 1', rotation: 0 },
        life: seat.life + pending,
        pending,
        line: byPlayer.get(seat.id),
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
    taps.bump(playerId, delta)
  }

  /**
   * Correct a seat's total outright. Any pending taps for that seat are discarded first —
   * they described a change *to* a number the user is now replacing, so committing them after
   * the correction would move it again.
   */
  async function setLife(playerId: number, life: number) {
    if (!isActive.value) return
    taps.discard(playerId)
    await adjust.mutateAsync({
      game: game.value,
      sessionId: sessionId.value,
      playerId,
      change: { life },
    })
  }

  /** Record the result. Pending taps are flushed first so the final totals are the real ones. */
  async function finishGame(winnerPlayerId: number | null) {
    taps.flush()
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
    bump,
    setLife,
    finishGame,
    undoEvent,
    isFinishing: computed(() => finish.isPending.value),
    isUndoing: computed(() => undo.isPending.value),
  }
}
