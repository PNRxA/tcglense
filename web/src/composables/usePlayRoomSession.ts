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
 *      the server hands back the same seat. If it rejects the token (the seat was removed, the
 *      room was recreated on the same code), we forget it and fall through to (2)/(3) rather
 *      than stranding the player on an error they can't act on.
 *   2. **Signed in** — join with no name: the server matches the seat by user id and re-issues
 *      its token, so the same account gets its seat back from any device.
 *   3. **A guest with no token** — we cannot join for them: a seat needs a display name, so
 *      `needsName` goes true and the view shows the join card. `join(name)` finishes the job.
 *
 * A full or started room is not a failure either — a viewer with no seat may still `watch()`,
 * which connects the socket with a null token (the server's spectator view).
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
  /** Connect with no seat, as a spectator. */
  watch: () => void
  /** Forget this browser's seat (after leaving) and go back to resolving. */
  clearSeat: () => void
}

export function usePlayRoomSession(game: Ref<string>, code: Ref<string>): PlayRoomSession {
  const auth = useAuthStore()
  const store = usePlayRoomStore()
  const { connection, room: pushedRoom, closedReason } = storeToRefs(store)

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

    const remembered = recallSeatToken(game.value, code.value)
    if (remembered) {
      try {
        await attempt({ seat_token: remembered })
        return
      } catch {
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
        error.value = (err as ApiError).message
        phase.value = 'blocked'
        return
      }
    }

    // A guest: we have nothing to join with until they tell us who they are.
    phase.value = 'needs-name'
  }

  async function join(name?: string): Promise<void> {
    error.value = null
    try {
      await attempt(name ? { name } : {})
    } catch (err) {
      error.value = (err as ApiError).message
      // Stay on the join card so the name can be corrected (or a different one tried); only a
      // signed-in caller, who had no form to begin with, is left with nothing to do.
      if (!auth.isAuthenticated) phase.value = 'needs-name'
      else phase.value = 'blocked'
    }
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
    watch: watchOnly,
    clearSeat,
  }
}
