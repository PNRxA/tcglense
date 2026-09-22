import { computed, onScopeDispose, ref, watch, type ComputedRef, type Ref } from 'vue'
import { storeToRefs } from 'pinia'
import type { ApiError, PlayJoinResponse, PlayRoomSummary, PlaySeatView } from '@/lib/api'
import { forgetSeatToken, recallSeatToken, rememberSeatToken } from '@/lib/playSeat'
import { usePlayRoomQuery, useJoinPlayRoom } from '@/composables/usePlayRooms'
import { usePlayRoomStore } from '@/stores/playRoom'
import { useAuthStore } from '@/stores/auth'

/**
 * The room page's engine: get this browser to a seat, then get that seat onto the socket.
 *
 * Everything hard about the play table's *entry* is here, because the answer to "who are you"
 * has three shapes and the view should render one state machine rather than branch on all of
 * them:
 *
 *   1. **We've been here before** — a token remembered for this `(game, code)`. Replay it and
 *      the server hands back the same seat. A **definitive** rejection (401 the token names no
 *      seat, 404 the room is gone) forgets it and falls through to (2)/(3) rather than
 *      stranding the player on an error they can't act on. Anything else — offline, a 5xx, a
 *      timeout — says nothing about the seat, so the token is **kept** and the view offers
 *      `retry()`: falling through there would join a second time and sit the same guest at the
 *      table twice, with the seat they actually hold still occupied and unreachable.
 *   2. **Signed in** — join with no name: the server matches the seat by user id and re-issues
 *      its token, so the same account gets its seat back from any device.
 *   3. **A guest with no token** — we cannot join for them: a seat needs a display name, so
 *      `needsName` goes true and the view shows the join card. `join(name)` finishes the job.
 *
 * A full or started room is not a failure either — a viewer with no seat may still `watch()`,
 * which connects the socket with a null token (the server's spectator view).
 *
 * A close is part of the same funnel. The socket reports why it gave up, and only the
 * **code** distinguishes a recoverable rotation from an ending: `4003` (the token names no
 * seat of this room) is what a signed-in player gets when their seat's token was re-issued
 * elsewhere, so we forget it and re-join — the server matches the seat by user id and hands
 * back a fresh token — while a guest, whose only proof it was, goes back to the join card.
 * Every other final code (`4004` the room or seat is gone, `4008` flooding) is the end, and
 * the view says so. `1011` is a server hiccup and never reaches here at all: it is outside the
 * `4xxx` range, so `PlaySocket` reconnects on its own.
 *
 * The socket is the store's; this composable only decides *what token it opens with* and tears
 * it down on the way out. The room summary is polled only while the socket isn't carrying
 * lobby pushes (see `usePlayRoomQuery`), so the lobby has exactly one live source at a time.
 */

export type PlaySeatPhase =
  /** Working out which seat (if any) is ours. */
  | 'resolving'
  /** A guest with no remembered seat: the view must ask for a name. */
  | 'needs-name'
  /** We hold a seat and the socket is up (or coming up). */
  | 'seated'
  /** Connected with no seat — watching. */
  | 'watching'
  /** Nothing to join and nothing to watch (unknown code, or the join failed). */
  | 'blocked'
  /** We hold a seat we couldn't replay *right now* (offline, a 5xx): the token is kept. */
  | 'retry'

export interface PlayRoomSession {
  phase: Ref<PlaySeatPhase>
  /** The room as last read or pushed — the poll's answer until the socket takes over. */
  room: ComputedRef<PlayRoomSummary | null>
  seat: Ref<PlaySeatView | null>
  seatId: ComputedRef<number | null>
  /** The seat's token: what every seat-scoped write is keyed by. */
  seatToken: Ref<string | null>
  /** True while the initial by-code read is in flight and we have nothing to show. */
  isLoading: ComputedRef<boolean>
  /** The code addresses no room (a typo, or a room that was closed). */
  notFound: ComputedRef<boolean>
  /** Whatever stopped us getting a seat, in the server's words. */
  error: Ref<string | null>
  /** Why the server closed the socket for good (room deleted, seat removed …). */
  closedReason: Ref<string | null>
  isJoining: Readonly<Ref<boolean>>
  /** Take a seat. `name` is required for a guest and ignored for a signed-in caller. */
  join: (name?: string) => Promise<void>
  /** Try the remembered seat again after a transient failure (the `retry` phase). */
  retry: () => void
  /** Connect with no seat, as a spectator. */
  watch: () => void
  /** Forget this browser's seat (after leaving) and go back to resolving. */
  clearSeat: () => void
}

/** A seat token the server has *definitively* disowned, as opposed to one it couldn't answer for. */
function isTokenRejection(err: unknown): boolean {
  const status = (err as ApiError | null)?.status
  return status === 401 || status === 404
}

/** Whatever the failure has to say for itself, in words the join card can show. */
function messageOf(err: unknown): string {
  const message = (err as { message?: unknown } | null)?.message
  return typeof message === 'string' && message ? message : 'the table could not be reached'
}

/** The close code the server uses for "this token names no seat of mine" — recoverable. */
const CLOSE_BAD_SEAT_TOKEN = 4003

export function usePlayRoomSession(game: Ref<string>, code: Ref<string>): PlayRoomSession {
  const auth = useAuthStore()
  const store = usePlayRoomStore()
  const { connection, room: pushedRoom, closedReason, closedCode } = storeToRefs(store)

  /** How many times one visit may answer a rotated token by joining again (see below). */
  const MAX_TOKEN_ROTATIONS = 1
  let rotations = 0

  const phase = ref<PlaySeatPhase>('resolving')
  const seat = ref<PlaySeatView | null>(null)
  const seatToken = ref<string | null>(null)
  const error = ref<string | null>(null)

  const socketOpen = computed(() => connection.value === 'open')
  const query = usePlayRoomQuery(game, code, { socketOpen })
  const joinRoom = useJoinPlayRoom()

  /** The pushed lobby wins once the socket has sent one — it is newer than any poll. */
  const room = computed<PlayRoomSummary | null>(() => pushedRoom.value ?? query.data.value ?? null)
  const notFound = computed(() => query.error.value?.status === 404)
  const isLoading = computed(() => query.isPending.value && !room.value)
  const seatId = computed(() => seat.value?.id ?? null)

  /**
   * Keep the seat view fresh from whichever lobby is current — the seat row carries the deck
   * and ready flags the lobby renders, and a stale copy would show a deck the player has
   * already replaced.
   */
  watch(
    () => [room.value, seat.value?.id] as const,
    ([current, id]) => {
      if (!current || id == null) return
      const fresh = current.seats.find((s) => s.id === id)
      if (fresh) seat.value = fresh
    },
  )

  function adopt(result: PlayJoinResponse): void {
    seat.value = result.seat
    seatToken.value = result.seat_token
    rememberSeatToken(game.value, code.value, result.seat_token)
    error.value = null
    phase.value = 'seated'
    store.connect(game.value, code.value.toUpperCase(), result.seat_token)
  }

  /** Connect with no seat: the public view of the table. */
  function watchOnly(): void {
    seat.value = null
    seatToken.value = null
    phase.value = 'watching'
    store.connect(game.value, code.value.toUpperCase(), null)
  }

  async function attempt(body: { name?: string; seat_token?: string }): Promise<void> {
    const result = await joinRoom.mutateAsync({ game: game.value, code: code.value, body })
    adopt(result)
  }

  /** The resolution order in the module docs, run once per `(game, code)`. */
  async function resolve(): Promise<void> {
    phase.value = 'resolving'
    error.value = null
    rotations = 0

    const remembered = recallSeatToken(game.value, code.value)
    if (remembered) {
      try {
        await attempt({ seat_token: remembered })
        return
      } catch (err) {
        if (!isTokenRejection(err)) {
          // The server didn't say the token is bad — it didn't answer at all (offline, a 5xx,
          // a timeout). The seat is very probably still ours, so keep the token and offer the
          // one useful verb. Joining afresh here would take a *second* seat and strand the
          // first, which is the one failure a player cannot undo themselves.
          error.value = messageOf(err)
          phase.value = 'retry'
          return
        }
        // A token the server won't take is worse than none — it would fail every retry and
        // then fail the socket's hello too. Drop it and try to get a seat the normal way.
        forgetSeatToken(game.value, code.value)
      }
    }

    if (auth.isAuthenticated) {
      try {
        await attempt({})
        return
      } catch (err) {
        error.value = messageOf(err)
        phase.value = 'blocked'
        return
      }
    }

    // A guest: we have nothing to join with until they tell us who they are.
    phase.value = 'needs-name'
  }

  async function join(name?: string): Promise<void> {
    error.value = null
    rotations = 0
    try {
      await attempt(name ? { name } : {})
    } catch (err) {
      error.value = messageOf(err)
      // Stay on the join card so the name can be corrected (or a different one tried); only a
      // signed-in caller, who had no form to begin with, is left with nothing to do.
      if (!auth.isAuthenticated) phase.value = 'needs-name'
      else phase.value = 'blocked'
    }
  }

  /**
   * A `4003` close: the token this browser holds names no seat of this room.
   *
   * For a **signed-in** player that is routine rather than fatal — a join re-issues the seat's
   * token and matches the seat by user id, so opening the same room on a phone rotates the
   * laptop's token out from under it. Forget it, join again (which hands back the same seat
   * with a fresh token) and reconnect; telling them the room is gone would be a lie they can't
   * act on. A **guest** has no account to re-derive a seat from, so their proof is simply gone
   * and the join card is the honest answer.
   *
   * Bounded to {@link MAX_TOKEN_ROTATIONS} per visit: if the re-join's own token is refused
   * too, something else is wrong and a loop of joins is the worst possible response.
   */
  async function recoverRotatedSeat(): Promise<void> {
    forgetSeatToken(game.value, code.value)
    seatToken.value = null
    // Not an ending, so nothing on screen should say it was one.
    store.clearClosed()
    if (!auth.isAuthenticated) {
      seat.value = null
      store.reset()
      phase.value = 'needs-name'
      return
    }
    phase.value = 'resolving'
    try {
      await attempt({})
    } catch (err) {
      error.value = messageOf(err)
      phase.value = 'blocked'
    }
  }

  watch(closedCode, (current) => {
    if (current !== CLOSE_BAD_SEAT_TOKEN) return
    // Out of budget: leave the close as the server reported it, and let the view say so.
    if (rotations >= MAX_TOKEN_ROTATIONS) return
    rotations += 1
    void recoverRotatedSeat()
  })

  /** The `retry` phase's one verb: the token is still ours, so just run the funnel again. */
  function retry(): void {
    void resolve()
  }

  function clearSeat(): void {
    forgetSeatToken(game.value, code.value)
    seat.value = null
    seatToken.value = null
    store.reset()
    phase.value = auth.isAuthenticated ? 'blocked' : 'needs-name'
  }

  // Resolve on arrival and whenever the addressed room changes (a player can walk from one
  // room to another without the view unmounting). Waits for the session to be resolved, so a
  // signed-in player reloading the page isn't mistaken for a guest.
  watch(
    () => [game.value, code.value.toUpperCase(), auth.sessionResolved] as const,
    ([, nextCode, resolved], previous) => {
      if (!resolved || !nextCode) return
      if (previous && previous[0] === game.value && previous[1] === nextCode && previous[2]) return
      store.reset()
      void resolve()
    },
    { immediate: true },
  )

  // Leaving the page drops the socket; the seat token survives in localStorage so coming back
  // lands on the same seat.
  onScopeDispose(() => store.reset())

  return {
    phase,
    room,
    seat,
    seatId,
    seatToken,
    isLoading,
    notFound,
    error,
    closedReason,
    isJoining: joinRoom.isPending,
    join,
    retry,
    watch: watchOnly,
    clearSeat,
  }
}
