import type { Ref } from 'vue'
import { useQueryClient, type QueryClient } from '@tanstack/vue-query'
import {
  addLifePlayer,
  adjustLife,
  deleteLifeSession,
  finishLifeSession,
  getLifeDeckRecords,
  getLifeSession,
  getLifeSessions,
  removeLifePlayer,
  reorderLifePlayers,
  startLifeSession,
  undoLifeEvent,
  updateLifePlayer,
  updateLifeSession,
  type LifeLayout,
  type LifeRotation,
  type LifeSessionStatus,
  type NewLifeSeat,
  type StartLifeSessionBody,
} from '@/lib/api/life'
import type {
  ApiError,
  LifeChange,
  LifeDeckRecord,
  LifeSeat,
  LifeSession,
  LifeSessionDetail,
} from '@/lib/api'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'

// ---------- Life-counter query + mutation composables ----------
//
// The life counter is a per-user *container* surface like decks — many tracked games per
// game, each with seats and a life history — so it lives beside the holdings factories
// rather than inside them, mirroring `useDecks.ts`'s idioms: every key family starts with
// `life` so `useAuthCacheReset` wipes it on an identity change, reactive params go INSIDE the
// query key as refs, and each option object is an intermediate variable with explicitly typed
// callbacks so TanStack's deeply-reactive types don't trip excess-property checks through the
// `useAuthed*` wrappers.
//
// What's different from decks: most writes return the **whole session** and are written
// straight into the cache with `setQueryData` instead of invalidating. A life counter is a
// live surface — a refetch round trip after every change would make the totals visibly lag
// the taps — and the server's response is already the complete authoritative state, so
// adopting it is both faster and no less correct. The list is invalidated separately, since
// it's a different shape.

/** The query key for one tracked game's detail — shared by the reads and the cache patches. */
export function lifeSessionKey(game: string, sessionId: number) {
  return ['life-session', game, sessionId] as const
}

// ----- Reads -----

/** The caller's tracked games for a game, newest first. `status` narrows to in-progress. */
export function useLifeSessionsQuery(
  game: Ref<string>,
  status?: Ref<LifeSessionStatus | undefined>,
) {
  const options = {
    queryKey: ['life-sessions', game, status],
    queryFn: (token: string) => getLifeSessions(token, game.value, { status: status?.value }),
  }
  return useAuthedQuery<{ data: LifeSession[] }>(options)
}

/**
 * One tracked game in full. `sessionId` is a ref inside the key, so navigating between games
 * refetches.
 *
 * `staleTime: 0` and no refetch-on-focus: this is the live game, and every write already
 * writes the authoritative response into the cache, so background refetching would only
 * risk clobbering a just-committed total with an in-flight older read.
 */
export function useLifeSessionQuery(
  game: Ref<string>,
  sessionId: Ref<number>,
  enabled?: Ref<boolean>,
) {
  const options = {
    queryKey: ['life-session', game, sessionId],
    queryFn: (token: string) => getLifeSession(token, game.value, sessionId.value),
    enabled,
    staleTime: 0,
    refetchOnWindowFocus: false,
  }
  return useAuthedQuery<LifeSessionDetail>(options)
}

/** Per-deck win/loss records across finished games. `deckId` narrows to one deck. */
export function useLifeDeckRecordsQuery(
  game: Ref<string>,
  deckId?: Ref<number | undefined>,
  enabled?: Ref<boolean>,
) {
  const options = {
    queryKey: ['life-deck-records', game, deckId],
    queryFn: (token: string) => getLifeDeckRecords(token, game.value, deckId?.value),
    enabled,
  }
  return useAuthedQuery<{ data: LifeDeckRecord[] }>(options)
}

// ----- Cache maintenance -----

/** Adopt a write's authoritative session into the cache, and refresh the derived lists. */
export function adoptSession(qc: QueryClient, game: string, detail: LifeSessionDetail) {
  qc.setQueryData(lifeSessionKey(game, detail.session.id), detail)
  qc.invalidateQueries({ queryKey: ['life-sessions', game] })
  // A finished game changes the per-deck record.
  qc.invalidateQueries({ queryKey: ['life-deck-records', game] })
}

/**
 * Fold one life change into the cached session: swap in the authoritative seat and append the
 * event.
 *
 * The hot path deliberately doesn't adopt a whole session (the commit response is just the
 * seat + its event, so the round trip stays small), which means the patch has to be idempotent
 * against a concurrent read: an event id already present is not appended twice.
 */
export function applyLifeChange(
  qc: QueryClient,
  game: string,
  sessionId: number,
  change: LifeChange,
) {
  qc.setQueryData<LifeSessionDetail>(lifeSessionKey(game, sessionId), (current) => {
    if (!current) return current
    return {
      session: {
        ...current.session,
        players: current.session.players.map((seat) =>
          seat.id === change.player.id ? change.player : seat,
        ),
      },
      events: current.events.some((event) => event.id === change.event.id)
        ? current.events
        : [...current.events, change.event],
    }
  })
}

/** Refresh the session list (and, after a delete, drop the detached detail entry). */
export function invalidateLifeSessions(qc: QueryClient, game: string) {
  qc.invalidateQueries({ queryKey: ['life-sessions', game] })
}

// ----- Mutation variable shapes -----

export interface StartSessionVars {
  game: string
  body: StartLifeSessionBody
}
export interface SessionVars {
  game: string
  sessionId: number
}
export interface UpdateSessionVars extends SessionVars {
  body: { name?: string; format?: string; layout?: LifeLayout }
}
export interface FinishSessionVars extends SessionVars {
  winnerPlayerId: number | null
}
export interface AddPlayerVars extends SessionVars {
  body: NewLifeSeat
}
export interface UpdatePlayerVars extends SessionVars {
  playerId: number
  body: {
    name: string
    deck_id?: number | null
    commander_card_id?: string | null
    rotation?: LifeRotation
  }
}
export interface PlayerVars extends SessionVars {
  playerId: number
}
export interface ReorderPlayersVars extends SessionVars {
  playerIds: number[]
}
export interface AdjustLifeVars extends SessionVars {
  playerId: number
  change: { delta: number } | { life: number }
}
export interface UndoEventVars extends SessionVars {
  eventId: number
}

// ----- Mutations -----

export function useStartLifeSessionMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: StartSessionVars) =>
      startLifeSession(token, vars.game, vars.body),
    onSuccess: (detail: LifeSessionDetail, vars: StartSessionVars) =>
      adoptSession(qc, vars.game, detail),
  }
  return useAuthedMutation<LifeSessionDetail, StartSessionVars>(options)
}

export function useUpdateLifeSessionMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: UpdateSessionVars) =>
      updateLifeSession(token, vars.game, vars.sessionId, vars.body),
    // The header write returns the session without its history, so patch the header in place
    // rather than adopting (which would drop the events).
    onSuccess: (session: LifeSession, vars: UpdateSessionVars) => {
      qc.setQueryData<LifeSessionDetail>(lifeSessionKey(vars.game, vars.sessionId), (current) =>
        current ? { ...current, session } : current,
      )
      invalidateLifeSessions(qc, vars.game)
    },
  }
  return useAuthedMutation<LifeSession, UpdateSessionVars>(options)
}

export function useFinishLifeSessionMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: FinishSessionVars) =>
      finishLifeSession(token, vars.game, vars.sessionId, vars.winnerPlayerId),
    onSuccess: (detail: LifeSessionDetail, vars: FinishSessionVars) =>
      adoptSession(qc, vars.game, detail),
  }
  return useAuthedMutation<LifeSessionDetail, FinishSessionVars>(options)
}

export function useDeleteLifeSessionMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: SessionVars) =>
      deleteLifeSession(token, vars.game, vars.sessionId),
    onSuccess: (_d: void, vars: SessionVars) => {
      qc.removeQueries({ queryKey: lifeSessionKey(vars.game, vars.sessionId) })
      invalidateLifeSessions(qc, vars.game)
      qc.invalidateQueries({ queryKey: ['life-deck-records', vars.game] })
    },
  }
  return useAuthedMutation<void, SessionVars>(options)
}

export function useAddLifePlayerMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: AddPlayerVars) =>
      addLifePlayer(token, vars.game, vars.sessionId, vars.body),
    onSuccess: (detail: LifeSessionDetail, vars: AddPlayerVars) =>
      adoptSession(qc, vars.game, detail),
  }
  return useAuthedMutation<LifeSessionDetail, AddPlayerVars>(options)
}

export function useUpdateLifePlayerMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: UpdatePlayerVars) =>
      updateLifePlayer(token, vars.game, vars.sessionId, vars.playerId, vars.body),
    onSuccess: (seat: LifeSeat, vars: UpdatePlayerVars) => {
      qc.setQueryData<LifeSessionDetail>(lifeSessionKey(vars.game, vars.sessionId), (current) =>
        current
          ? {
              ...current,
              session: {
                ...current.session,
                // This endpoint owns the seat's metadata, not its `life` — the total it echoes is
                // whatever the row held when it was read. Keeping the cached life means a life
                // write that resolves in the other order can't be reverted on screen by a rename.
                players: current.session.players.map((p) =>
                  p.id === seat.id ? { ...seat, life: p.life } : p,
                ),
              },
            }
          : current,
      )
      invalidateLifeSessions(qc, vars.game)
    },
  }
  return useAuthedMutation<LifeSeat, UpdatePlayerVars>(options)
}

export function useRemoveLifePlayerMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: PlayerVars) =>
      removeLifePlayer(token, vars.game, vars.sessionId, vars.playerId),
    onSuccess: (detail: LifeSessionDetail, vars: PlayerVars) => adoptSession(qc, vars.game, detail),
  }
  return useAuthedMutation<LifeSessionDetail, PlayerVars>(options)
}

export function useReorderLifePlayersMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: ReorderPlayersVars) =>
      reorderLifePlayers(token, vars.game, vars.sessionId, vars.playerIds),
    onSuccess: (detail: LifeSessionDetail, vars: ReorderPlayersVars) =>
      adoptSession(qc, vars.game, detail),
  }
  return useAuthedMutation<LifeSessionDetail, ReorderPlayersVars>(options)
}

/**
 * Commit one life change. Called by the tap engine (`useLifeSession`) after it has batched a
 * run of taps into a single delta, so this is one request per *committed* change, not per tap.
 */
export function useAdjustLifeMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: AdjustLifeVars) =>
      adjustLife(token, vars.game, vars.sessionId, vars.playerId, vars.change),
    onSuccess: (change: LifeChange, vars: AdjustLifeVars) =>
      applyLifeChange(qc, vars.game, vars.sessionId, change),
  }
  return useAuthedMutation<LifeChange, AdjustLifeVars>(options)
}

export function useUndoLifeEventMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: UndoEventVars) =>
      undoLifeEvent(token, vars.game, vars.sessionId, vars.eventId),
    onSuccess: (detail: LifeSessionDetail, vars: UndoEventVars) =>
      adoptSession(qc, vars.game, detail),
  }
  return useAuthedMutation<LifeSessionDetail, UndoEventVars>(options)
}

/** Re-exported so views can name the error type without reaching into the client. */
export type { ApiError }
