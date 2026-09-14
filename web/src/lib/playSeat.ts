/**
 * Seat-token persistence for the play table.
 *
 * A seat token is the only proof a player holds a seat: the server shows it once (on join)
 * and stores nothing but its hash, so losing it means losing the seat. A *guest* has no
 * account to re-derive it from — a refresh, a closed tab, or a phone locking mid-game would
 * otherwise drop them from their own game — so the token is remembered per `(game, code)`
 * in localStorage and replayed as `seat_token` on the next join.
 *
 * Per room, not one key: a player can hold a seat in several rooms at once, and one room's
 * token is useless in another. Signed-in players get the same treatment because it also saves
 * them a token rotation on every reload.
 *
 * Reads and writes are guarded exactly like `lib/persistedRef.ts`: blocked or full storage
 * (private mode, a strict browser) must degrade to "this tab only", never throw — a table
 * that works until you refresh beats one that won't load.
 */

const PREFIX = 'tcglense_play_seat'

/** The storage key for one room. Codes are case-insensitive on the wire; normalise here. */
export function playSeatKey(game: string, code: string): string {
  return `${PREFIX}:${game}:${code.toUpperCase()}`
}

/** Remember the token a join answered with. A blank token forgets the seat instead. */
export function rememberSeatToken(game: string, code: string, token: string): void {
  if (!token) {
    forgetSeatToken(game, code)
    return
  }
  try {
    localStorage.setItem(playSeatKey(game, code), token)
  } catch {
    // Storage unavailable: the seat still works for this tab (the token lives in memory).
  }
}

/** The token last remembered for this room, or null. */
export function recallSeatToken(game: string, code: string): string | null {
  try {
    const stored = localStorage.getItem(playSeatKey(game, code))
    return stored ? stored : null
  } catch {
    // Storage unavailable: behave as if we'd never seen this room.
    return null
  }
}

/** Drop the remembered token — on leaving the seat, or when the server rejects it. */
export function forgetSeatToken(game: string, code: string): void {
  try {
    localStorage.removeItem(playSeatKey(game, code))
  } catch {
    // Nothing to do: an unreadable store is also an unwritable one.
  }
}
