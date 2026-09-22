import { computed, toValue, type MaybeRefOrGetter, type Ref } from 'vue'
import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/vue-query'
import {
  createPlayRoom,
  deletePlayRoom,
  getPlayRoom,
  joinPlayRoom,
  leavePlaySeat,
  listPlayRooms,
  loadPlaySeatDeck,
  setPlaySeatReady,
  type CreatePlayRoomBody,
  type JoinPlayRoomBody,
  type LoadPlayDeckBody,
} from '@/lib/api/play'
import type {
  ApiError,
  PlayJoinResponse,
  PlayRoomStatus,
  PlayRoomSummary,
  PlaySeatView,
} from '@/lib/api'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'
import { useAuthStore } from '@/stores/auth'

// ---------- Play-table query + mutation composables ----------
//
// The play table is the one surface in the app where a *caller may have no account at all*:
// a guest follows an invite link, gives a name, and holds a seat proved by a per-seat token
// rather than a session. So this file is deliberately split down the middle:
//
//   * "Your rooms" and creating/deleting a room are the host's, and are plain
//     `useAuthedQuery`/`useAuthedMutation` like every other per-user family.
//   * Reading a room by code, joining it, and everything seat-scoped are `useQuery` /
//     `useMutation` with the session token passed only when there IS one — a guest's
//     `authFetch` would simply never run, which would break the feature for exactly the
//     people it exists for.
//
// Every key family starts with `play` so `useAuthCacheReset` wipes the per-user half on an
// identity change, and reactive params go INSIDE the keys as refs (never `.value`).
//
// The live table is NOT here: once the socket is open the server pushes lobby and table state
// (`stores/playRoom.ts`), and a poll would fight it. That's what `usePlayRoomQuery`'s
// `socketOpen` flag is for — the poll is the fallback for the window before the socket
// connects (and while it is reconnecting), not a second source of truth.

/** The room-detail key, spelled once so the mutations can seed and invalidate it. */
export function playRoomKey(game: string, code: string) {
  return ['play-room', game, code.toUpperCase()] as const
}

/** How often the by-code read re-polls while the socket is *not* carrying lobby pushes. */
export const PLAY_ROOM_POLL_MS = 5_000

// ----- Reads -----

/**
 * Rooms the caller hosts or holds a seat in, most recently active first. Authed: a guest has
 * no list (their seats live only in `lib/playSeat.ts`'s per-room tokens).
 */
export function usePlayRoomsQuery(
  game: Ref<string>,
  status?: Ref<PlayRoomStatus | undefined>,
  enabled?: MaybeRefOrGetter<boolean>,
) {
  const options = {
    queryKey: ['play-rooms', game, status],
    queryFn: (token: string) => listPlayRooms(token, game.value, { status: status?.value }),
    enabled,
    // A lobby others are joining changes under you; keep it fresh but not chatty.
    staleTime: 10_000,
  }
  return useAuthedQuery<{ data: PlayRoomSummary[] }>(options)
}

/**
 * One room by its invite code — a PUBLIC read (the join page must render for a guest who has
 * never signed in), so `useQuery`, not `useAuthedQuery`.
 *
 * `socketOpen` turns the poll off: while the socket is open the server pushes a `lobby` frame
 * on every seat change, which is both faster and cheaper than re-asking. Pass it as a ref off
 * the store's `connection` and this becomes a poll only for the seconds before the socket
 * connects, or while it's away.
 */
export function usePlayRoomQuery(
  game: Ref<string>,
  code: Ref<string>,
  opts: { enabled?: MaybeRefOrGetter<boolean>; socketOpen?: MaybeRefOrGetter<boolean> } = {},
) {
  const polling = computed(() => (toValue(opts.socketOpen ?? false) ? false : PLAY_ROOM_POLL_MS))
  // Normalised in the key, not just on the wire: a link typed in lower case and one pasted in
  // upper case are the same room, and two cache entries for it would show two lobbies.
  const normalised = computed(() => code.value.toUpperCase())
  return useQuery<PlayRoomSummary, ApiError>({
    queryKey: ['play-room', game, normalised],
    queryFn: ({ signal }) => getPlayRoom(game.value, normalised.value, signal),
    enabled: computed(() => toValue(opts.enabled ?? true) && Boolean(code.value)),
    refetchInterval: polling,
    // The lobby is live state; nothing about it is worth holding as fresh.
    staleTime: 0,
    // A 404 is a wrong code, not a blip — the view says "no such room" rather than retrying.
    retry: false,
  })
}

// ----- Cache maintenance -----

/** Adopt a room a write answered with, and refresh the caller's list. */
export function adoptPlayRoom(qc: QueryClient, game: string, room: PlayRoomSummary) {
  qc.setQueryData(playRoomKey(game, room.code), room)
  qc.invalidateQueries({ queryKey: ['play-rooms', game] })
}

/** Swap one seat into the cached room summary — what the seat-scoped writes answer with. */
export function adoptPlaySeat(qc: QueryClient, game: string, code: string, seat: PlaySeatView) {
  qc.setQueryData<PlayRoomSummary>(playRoomKey(game, code), (current) =>
    current
      ? { ...current, seats: current.seats.map((s) => (s.id === seat.id ? seat : s)) }
      : current,
  )
}

// ----- Mutation variable shapes -----

export interface CreatePlayRoomVars {
  game: string
  body: CreatePlayRoomBody
}
export interface JoinPlayRoomVars {
  game: string
  code: string
  body: JoinPlayRoomBody
}
export interface PlaySeatVars {
  game: string
  code: string
  seatId: number
  seatToken: string
}
export interface LoadPlayDeckVars extends PlaySeatVars {
  body: LoadPlayDeckBody
}
export interface SetPlayReadyVars extends PlaySeatVars {
  ready: boolean
}
export interface LeavePlaySeatVars {
  game: string
  code: string
  seatId: number
  /** The seat's own token (leaving), or omitted when the host removes someone else. */
  seatToken?: string
}
export interface DeletePlayRoomVars {
  game: string
  code: string
}

// ----- Writes -----

/** Open a room. The caller is seated at seat 0 as host and gets that seat's token back. */
export function useCreatePlayRoom() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: CreatePlayRoomVars) =>
      createPlayRoom(token, vars.game, vars.body),
    onSuccess: (result: PlayJoinResponse, vars: CreatePlayRoomVars) =>
      adoptPlayRoom(qc, vars.game, result.room),
  }
  return useAuthedMutation<PlayJoinResponse, CreatePlayRoomVars>(options)
}

/**
 * Take (or re-take) a seat. A **plain** mutation on purpose: a guest has no session, so this
 * reads the auth store itself and only sends a token when one exists — routing it through
 * `useAuthedMutation` would gate the whole feature behind an account.
 */
export function useJoinPlayRoom() {
  const qc = useQueryClient()
  const auth = useAuthStore()
  return useMutation<PlayJoinResponse, ApiError, JoinPlayRoomVars>({
    mutationFn: (vars) =>
      auth.isAuthenticated
        ? auth.authFetch((token) => joinPlayRoom(vars.game, vars.code, vars.body, token))
        : joinPlayRoom(vars.game, vars.code, vars.body),
    onSuccess: (result, vars) => adoptPlayRoom(qc, vars.game, result.room),
  })
}

/**
 * Load a deck into a seat. The session token rides along when there is one because
 * `source: 'deck'` reads the caller's own decks; a precon or a pasted list needs none.
 */
export function useLoadPlaySeatDeck() {
  const qc = useQueryClient()
  const auth = useAuthStore()
  return useMutation<PlaySeatView, ApiError, LoadPlayDeckVars>({
    mutationFn: (vars) =>
      auth.isAuthenticated
        ? auth.authFetch((token) =>
            loadPlaySeatDeck(vars.game, vars.code, vars.seatId, vars.seatToken, vars.body, token),
          )
        : loadPlaySeatDeck(vars.game, vars.code, vars.seatId, vars.seatToken, vars.body),
    onSuccess: (seat, vars) => adoptPlaySeat(qc, vars.game, vars.code, seat),
  })
}

/** Flip a seat's ready check. Seat-token scoped, so it works for a guest. */
export function useSetPlaySeatReady() {
  const qc = useQueryClient()
  return useMutation<PlaySeatView, ApiError, SetPlayReadyVars>({
    mutationFn: (vars) =>
      setPlaySeatReady(vars.game, vars.code, vars.seatId, vars.seatToken, vars.ready),
    onSuccess: (seat, vars) => adoptPlaySeat(qc, vars.game, vars.code, seat),
  })
}

/**
 * Give up a seat — with its own token, or (removing someone else) as the host, whose session
 * the server accepts in the token's place.
 */
export function useLeavePlaySeat() {
  const qc = useQueryClient()
  const auth = useAuthStore()
  return useMutation<void, ApiError, LeavePlaySeatVars>({
    mutationFn: (vars) =>
      auth.isAuthenticated
        ? auth.authFetch((token) =>
            leavePlaySeat(vars.game, vars.code, vars.seatId, { seatToken: vars.seatToken, token }),
          )
        : leavePlaySeat(vars.game, vars.code, vars.seatId, { seatToken: vars.seatToken }),
    onSuccess: (_void, vars) => {
      qc.invalidateQueries({ queryKey: playRoomKey(vars.game, vars.code) })
      qc.invalidateQueries({ queryKey: ['play-rooms', vars.game] })
    },
  })
}

/** Host only: close the room for everyone (every socket is closed with a final 4xxx). */
export function useDeletePlayRoom() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: DeletePlayRoomVars) =>
      deletePlayRoom(token, vars.game, vars.code),
    onSuccess: (_void: void, vars: DeletePlayRoomVars) => {
      qc.removeQueries({ queryKey: playRoomKey(vars.game, vars.code) })
      qc.invalidateQueries({ queryKey: ['play-rooms', vars.game] })
    },
  }
  return useAuthedMutation<void, DeletePlayRoomVars>(options)
}
