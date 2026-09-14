import { API_URL, request } from './client'
import type { PlayJoinResponse, PlayRoomStatus, PlayRoomSummary, PlaySeatView } from './generated'

export type {
  PlayAction,
  PlayCardDef,
  PlayCardFace,
  PlayCardView,
  PlayClientMessage,
  PlayJoinResponse,
  PlayLogEntry,
  PlayLogKind,
  PlayPatch,
  PlayPeek,
  PlayPeekKind,
  PlayPhase,
  PlayPlacement,
  PlayRoomStatus,
  PlayRoomSummary,
  PlaySeatSnapshot,
  PlaySeatView,
  PlayServerMessage,
  PlaySnapshot,
  PlayTurn,
  PlayZone,
} from './generated'

/**
 * The play table's REST client (`/api/tools/{game}/play/rooms/...`).
 *
 * Two kinds of caller share it: the *host* (signed in — creating, listing and deleting rooms
 * take the session `token` first, like every other authed module) and a *seat*, which may be
 * a guest with no account at all. Seat-scoped calls are keyed by the per-seat token the join
 * answered with, sent as `X-Play-Seat` — never by the session — so a guest and a signed-in
 * player go through the same code path. The live table itself is the socket
 * (`lib/playSocket.ts`); this module only gets you to a seat with a deck.
 */

/** The formats a room can be created in — mirrors `play::types::FORMATS` on the server. */
export const PLAY_FORMATS = ['commander', 'constructed'] as const
export type PlayFormat = (typeof PLAY_FORMATS)[number]

/** Default starting life per format (mirrors `play::types::default_starting_life`). */
export const PLAY_DEFAULT_LIFE: Readonly<Record<PlayFormat, number>> = {
  commander: 40,
  constructed: 20,
}

export const PLAY_MIN_PLAYERS = 2
export const PLAY_MAX_PLAYERS = 6
/** Room codes are six characters from an unambiguous alphabet (no 0/O/1/I). */
export const PLAY_CODE_LENGTH = 6

export interface CreatePlayRoomBody {
  label?: string
  format: PlayFormat
  starting_life?: number
  max_players?: number
}

export interface JoinPlayRoomBody {
  /** Required for a guest (1–32 chars); a signed-in caller defaults to their username. */
  name?: string
  /** A previously issued seat token — re-joins that seat without minting a new one. */
  seat_token?: string
}

/** What a seat loads: one of the caller's decks, any precon, or a pasted list. */
export type LoadPlayDeckBody =
  | { source: 'deck'; deck_id: number }
  | { source: 'precon'; slug: string }
  | { source: 'text'; text: string }

export interface ListPlayRoomsParams {
  status?: PlayRoomStatus
  limit?: number
}

const base = (game: string): string => `/api/tools/${encodeURIComponent(game)}/play/rooms`
const roomBase = (game: string, code: string): string => `${base(game)}/${encodeURIComponent(code)}`
const seatBase = (game: string, code: string, seatId: number): string =>
  `${roomBase(game, code)}/seats/${seatId}`

/** The header a seat-scoped call carries its seat token in. */
export const PLAY_SEAT_HEADER = 'X-Play-Seat'

function seatHeaders(seatToken: string): Record<string, string> {
  return { [PLAY_SEAT_HEADER]: seatToken }
}

/** Open a room; the caller is seated at seat 0 as host. */
export function createPlayRoom(
  token: string,
  game: string,
  body: CreatePlayRoomBody,
): Promise<PlayJoinResponse> {
  return request<PlayJoinResponse>(base(game), { method: 'POST', body, token })
}

/** Rooms the caller hosts or holds a seat in, most recently active first. */
export function listPlayRooms(
  token: string,
  game: string,
  params: ListPlayRoomsParams = {},
): Promise<{ data: PlayRoomSummary[] }> {
  const query = new URLSearchParams()
  if (params.status) query.set('status', params.status)
  if (params.limit !== undefined) query.set('limit', String(params.limit))
  const qs = query.toString()
  return request<{ data: PlayRoomSummary[] }>(`${base(game)}${qs ? `?${qs}` : ''}`, { token })
}

/** A room by its invite code — public, so the join page can show who's at the table. */
export function getPlayRoom(
  game: string,
  code: string,
  signal?: AbortSignal,
): Promise<PlayRoomSummary> {
  return request<PlayRoomSummary>(roomBase(game, code), { signal })
}

/**
 * Take (or re-take) a seat. `token` is the session when signed in (so the seat is tied to
 * the account and re-joinable from any device); a guest passes none and a `name`.
 */
export function joinPlayRoom(
  game: string,
  code: string,
  body: JoinPlayRoomBody,
  token?: string | null,
): Promise<PlayJoinResponse> {
  return request<PlayJoinResponse>(`${roomBase(game, code)}/join`, {
    method: 'POST',
    body,
    token: token ?? undefined,
  })
}

/** Load a deck into a seat (lobby only). `token` is needed for `source: 'deck'`. */
export function loadPlaySeatDeck(
  game: string,
  code: string,
  seatId: number,
  seatToken: string,
  body: LoadPlayDeckBody,
  token?: string | null,
): Promise<PlaySeatView> {
  return request<PlaySeatView>(`${seatBase(game, code, seatId)}/deck`, {
    method: 'POST',
    body,
    token: token ?? undefined,
    headers: seatHeaders(seatToken),
  })
}

export function setPlaySeatReady(
  game: string,
  code: string,
  seatId: number,
  seatToken: string,
  ready: boolean,
): Promise<PlaySeatView> {
  return request<PlaySeatView>(`${seatBase(game, code, seatId)}/ready`, {
    method: 'POST',
    body: { ready },
    headers: seatHeaders(seatToken),
  })
}

/**
 * Leave a seat (its own token) or, as the host (session `token`), remove someone else's.
 * Lobby only.
 */
export function leavePlaySeat(
  game: string,
  code: string,
  seatId: number,
  opts: { seatToken?: string; token?: string | null },
): Promise<void> {
  return request<void>(seatBase(game, code, seatId), {
    method: 'DELETE',
    token: opts.token ?? undefined,
    headers: opts.seatToken ? seatHeaders(opts.seatToken) : undefined,
  })
}

/** Host only: close the room for everyone. */
export function deletePlayRoom(token: string, game: string, code: string): Promise<void> {
  return request<void>(roomBase(game, code), { method: 'DELETE', token })
}

/**
 * The room socket's URL. `API_URL` is normally empty (same-origin through the proxy), in
 * which case the page's own origin is used with the matching `ws`/`wss` scheme.
 */
export function playSocketUrl(
  game: string,
  code: string,
  location: { protocol: string; host: string } = window.location,
): string {
  const path = `${roomBase(game, code)}/ws`
  if (API_URL) {
    return `${API_URL.replace(/^http/, 'ws')}${path}`
  }
  const scheme = location.protocol === 'https:' ? 'wss' : 'ws'
  return `${scheme}://${location.host}${path}`
}
