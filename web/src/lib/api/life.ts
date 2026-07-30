import { request } from './client'
import type {
  LifeChange,
  LifeDeckRecord,
  LifeSeat,
  LifeSession,
  LifeSessionDetail,
} from './generated'

// ---------- Life counter (per-user, authenticated) ----------
//
// The first entry in the `/api/tools/{game}/...` namespace: a *session* is one tracked game
// of MTG, holding a seat per player, each seat's life total, and every change that got it
// there. Like decks (and unlike the collection/wish list) a user has many sessions per game,
// so the routes nest a `{sessionId}` and every seat/event call is scoped under it — the
// server proves the parent session is the caller's, so a foreign id is a 404.
//
// Every call takes an access `token` (via the auth store's `authFetch`); there is no public
// read here — a tracked game is private to its owner.

export type {
  AdjustLifeRequest,
  CreateLifeSessionRequest,
  FinishLifeSessionRequest,
  LifeChange,
  LifeDeckRecord,
  LifeEvent,
  LifeSeat,
  LifeSeatInput,
  LifeSession,
  LifeSessionDetail,
  ReorderLifeSeatsRequest,
  UpdateLifeSeatRequest,
  UpdateLifeSessionRequest,
} from './generated'

/** A session's lifecycle state: being played, or closed out with a result. */
export type LifeSessionStatus = 'active' | 'finished'

/** How a game ended for one seat. */
export type LifeSeatResult = 'none' | 'win' | 'loss' | 'draw'

/**
 * The seat-placement vocabulary, mirroring `LAYOUTS` in
 * `api/src/handlers/tools/life/mod.rs` — the server validates against its copy, so the two
 * lists must agree (pinned by a unit test on each side).
 */
export const LIFE_LAYOUTS = ['rows', 'facing', 'grid', 'pinwheel'] as const
export type LifeLayout = (typeof LIFE_LAYOUTS)[number]

/** A seat's screen rotation in degrees. */
export type LifeRotation = 0 | 90 | 180 | 270

/**
 * A seat as sent when starting a game or adding a player. Every field is optional — the
 * server names an unnamed seat `Player {n}`, inherits the session's starting life, and
 * rotates the seat the way the layout seats it.
 *
 * Spelled out here rather than reusing the generated `LifeSeatInput`: ts-rs renders a Rust
 * `Option<T>` as `T | null`, not `T?`, so the generated shape would force every caller to
 * pass explicit nulls for fields it doesn't care about.
 */
export interface NewLifeSeat {
  name?: string
  deck_id?: number | null
  /** The external card id of the commander this seat is playing — the alternative to a deck,
   * for an opponent whose deck you'll never have. Mutually exclusive with `deck_id`. */
  commander_card_id?: string | null
  starting_life?: number
  rotation?: LifeRotation
}

/** Body for starting a game (or rematching an earlier one). */
export interface StartLifeSessionBody {
  name?: string
  format?: string
  starting_life?: number
  layout?: LifeLayout
  players?: NewLifeSeat[]
  from_session_id?: number
}

const base = (game: string): string => `/api/tools/${encodeURIComponent(game)}/life`
const sessionBase = (game: string, sessionId: number): string =>
  `${base(game)}/sessions/${sessionId}`

// ----- Sessions -----

/** The caller's tracked games for a game, newest-started first (seats included, no history). */
export function getLifeSessions(
  token: string,
  game: string,
  options: { status?: LifeSessionStatus; limit?: number } = {},
): Promise<{ data: LifeSession[] }> {
  const query = new URLSearchParams()
  if (options.status) query.set('status', options.status)
  if (options.limit !== undefined) query.set('limit', String(options.limit))
  const suffix = query.size ? `?${query}` : ''
  return request<{ data: LifeSession[] }>(`${base(game)}/sessions${suffix}`, { token })
}

/** One tracked game in full: header, seats, and every recorded life change in order. */
export function getLifeSession(
  token: string,
  game: string,
  sessionId: number,
): Promise<LifeSessionDetail> {
  return request<LifeSessionDetail>(sessionBase(game, sessionId), { token })
}

/** Start a tracked game. Pass `from_session_id` alone to rematch an earlier table. */
export function startLifeSession(
  token: string,
  game: string,
  body: StartLifeSessionBody,
): Promise<LifeSessionDetail> {
  return request<LifeSessionDetail>(`${base(game)}/sessions`, { method: 'POST', body, token })
}

/** Edit a game's label, format or seat layout (each field optional, absent = unchanged). */
export function updateLifeSession(
  token: string,
  game: string,
  sessionId: number,
  body: { name?: string; format?: string; layout?: LifeLayout },
): Promise<LifeSession> {
  return request<LifeSession>(sessionBase(game, sessionId), { method: 'PUT', body, token })
}

/** Record the result: a winning seat id, or `null` for a draw across the table. */
export function finishLifeSession(
  token: string,
  game: string,
  sessionId: number,
  winnerPlayerId: number | null,
): Promise<LifeSessionDetail> {
  return request<LifeSessionDetail>(`${sessionBase(game, sessionId)}/finish`, {
    method: 'POST',
    body: { winner_player_id: winnerPlayerId },
    token,
  })
}

/** Delete a tracked game (its seats and whole life history cascade away). */
export function deleteLifeSession(token: string, game: string, sessionId: number): Promise<void> {
  return request<void>(sessionBase(game, sessionId), { method: 'DELETE', token })
}

// ----- Seats -----

/** Seat another player at the table. Returns the game, whose shape changed. */
export function addLifePlayer(
  token: string,
  game: string,
  sessionId: number,
  body: NewLifeSeat,
): Promise<LifeSessionDetail> {
  return request<LifeSessionDetail>(`${sessionBase(game, sessionId)}/players`, {
    method: 'POST',
    body,
    token,
  })
}

/**
 * Replace a seat's name, deck link and rotation. A full replace, not a patch — send the
 * fields you aren't changing as they stand.
 */
export function updateLifePlayer(
  token: string,
  game: string,
  sessionId: number,
  playerId: number,
  body: {
    name: string
    deck_id?: number | null
    commander_card_id?: string | null
    rotation?: LifeRotation
  },
): Promise<LifeSeat> {
  return request<LifeSeat>(`${sessionBase(game, sessionId)}/players/${playerId}`, {
    method: 'PUT',
    body,
    token,
  })
}

/** Take a seat off the table (the rest are renumbered). Returns the game. */
export function removeLifePlayer(
  token: string,
  game: string,
  sessionId: number,
  playerId: number,
): Promise<LifeSessionDetail> {
  return request<LifeSessionDetail>(`${sessionBase(game, sessionId)}/players/${playerId}`, {
    method: 'DELETE',
    token,
  })
}

/** Set the seat order (must be exactly the game's seats, each once). Returns the game. */
export function reorderLifePlayers(
  token: string,
  game: string,
  sessionId: number,
  playerIds: number[],
): Promise<LifeSessionDetail> {
  return request<LifeSessionDetail>(`${sessionBase(game, sessionId)}/players/reorder`, {
    method: 'PUT',
    body: { player_ids: playerIds },
    token,
  })
}

// ----- Life changes -----

/**
 * Move a seat's life and record it. Send exactly one of `delta` (a relative change — what a
 * run of taps commits) or `life` (an absolute correction).
 */
export function adjustLife(
  token: string,
  game: string,
  sessionId: number,
  playerId: number,
  change: { delta: number } | { life: number },
): Promise<LifeChange> {
  return request<LifeChange>(`${sessionBase(game, sessionId)}/players/${playerId}/life`, {
    method: 'POST',
    body: change,
    token,
  })
}

/** Undo one recorded change, from anywhere in the history. Returns the re-derived game. */
export function undoLifeEvent(
  token: string,
  game: string,
  sessionId: number,
  eventId: number,
): Promise<LifeSessionDetail> {
  return request<LifeSessionDetail>(`${sessionBase(game, sessionId)}/events/${eventId}`, {
    method: 'DELETE',
    token,
  })
}

// ----- Deck records -----

/**
 * Per-deck win/loss records across finished games, most-played first. `deckId` narrows to a
 * single deck — what a deck's own page asks for.
 */
export function getLifeDeckRecords(
  token: string,
  game: string,
  deckId?: number,
): Promise<{ data: LifeDeckRecord[] }> {
  const suffix = deckId === undefined ? '' : `?deck_id=${deckId}`
  return request<{ data: LifeDeckRecord[] }>(`${base(game)}/decks${suffix}`, { token })
}
