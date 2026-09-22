import { computed, ref, shallowRef, triggerRef } from 'vue'
import { defineStore } from 'pinia'
import type {
  PlayAction,
  PlayCardView,
  PlayLogEntry,
  PlayPatch,
  PlayPeek,
  PlayRoomStatus,
  PlayRoomSummary,
  PlaySeatSnapshot,
  PlayServerMessage,
  PlaySnapshot,
  PlayTurn,
  PlayZone,
} from '@/lib/api/play'
import { playSocketUrl } from '@/lib/api/play'
import { PlaySocket, type PlayConnection, type SocketFactory } from '@/lib/playSocket'

/**
 * The live table as this browser sees it — the client half of the play socket.
 *
 * Client state, not server state: the server pushes a `snapshot` when we say hello and a
 * `patch` per applied action, and this store folds them in. Nothing here is fetched with
 * vue-query (a socket is not a query), which is why it is a Pinia store and why
 * `useAuthCacheReset` doesn't need to know about it — `disconnect()` on leaving the page
 * is the whole lifecycle.
 *
 * Versioning: every patch carries the version it produces; a patch that isn't `local + 1`
 * means a frame was lost and we ask for a `resync` (a fresh snapshot) rather than guessing.
 *
 * Two kinds of room state live here: the **lobby** (`room`, a `PlayRoomSummary` the server
 * re-pushes on every seat change) and the **table** (`seats` / `cards` / `turn` / `log`,
 * present once the game has started). `status` reads off whichever is current.
 */

export interface PlayError {
  id: number | null
  code: string
  message: string
}

export const usePlayRoomStore = defineStore('playRoom', () => {
  // ---- identity ----
  const game = ref('')
  const code = ref('')
  const connection = ref<PlayConnection>('idle')
  /** The seat this connection holds (`null` = spectator, or not yet told). */
  const mySeatId = ref<number | null>(null)
  /** Why the server closed us for good (bad token, room deleted …), or null. */
  const closedReason = ref<string | null>(null)
  /**
   * The close code that came with it (`4003` a token this room doesn't know, `4004` the room
   * or seat is gone, `4008` flooding), or null while we're connected.
   *
   * The reason is prose for the player; the code is what the *session* acts on — a rotated
   * token is recoverable by re-joining, a deleted room is not, and only the code tells them
   * apart (`composables/usePlayRoomSession.ts`).
   */
  const closedCode = ref<number | null>(null)

  // ---- lobby ----
  const room = ref<PlayRoomSummary | null>(null)

  // ---- table ----
  const version = ref(0)
  const tableStatus = ref<PlayRoomStatus | null>(null)
  const format = ref('')
  const startingLife = ref(0)
  const seats = ref<PlaySeatSnapshot[]>([])
  /** Every card this viewer may see, by instance id. Shallow: patches replace entries. */
  const cards = shallowRef(new Map<number, PlayCardView>())
  const turn = ref<PlayTurn | null>(null)
  const log = ref<PlayLogEntry[]>([])
  const winner = ref<number | null>(null)
  /** The last `look_top` / `search_library` answer, until the UI clears it. */
  const peek = ref<PlayPeek | null>(null)
  const lastError = ref<PlayError | null>(null)
  /** A monotonically increasing id stamped on outgoing actions, echoed on `error`. */
  let nextActionId = 1
  /**
   * True between asking for a `resync` and the snapshot that answers it.
   *
   * A version gap is rarely one frame: a tab that was throttled in the background comes back
   * to a *burst* of patches, every one of which is out of order. One `resync` per gap fixes
   * all of them — asking again per patch would send dozens, which the server answers with
   * `error { code: "too_fast" }` (one resync per 2s) and, past `MAX_CONSECUTIVE_REJECTS`,
   * a `4008` close. So while one is in flight the rest of the burst is dropped in silence;
   * the snapshot that lands is the whole truth anyway.
   */
  let resyncPending = false

  const status = computed<PlayRoomStatus | null>(
    () => tableStatus.value ?? room.value?.status ?? null,
  )
  const hasTable = computed(() => tableStatus.value !== null)
  const mySeat = computed(() => seats.value.find((s) => s.id === mySeatId.value) ?? null)
  const isHost = computed(() => {
    if (mySeat.value) return mySeat.value.is_host
    return room.value?.seats.find((s) => s.id === mySeatId.value)?.is_host ?? false
  })
  /** Seats in table order starting after mine (so opponents render clockwise from me). */
  const opponents = computed(() => {
    const all = seats.value
    const idx = all.findIndex((s) => s.id === mySeatId.value)
    if (idx < 0) return all
    return [...all.slice(idx + 1), ...all.slice(0, idx)]
  })
  const isMyTurn = computed(
    () => mySeatId.value !== null && turn.value?.active_seat === mySeatId.value,
  )

  function seatById(seatId: number): PlaySeatSnapshot | undefined {
    return seats.value.find((s) => s.id === seatId)
  }

  function card(id: number): PlayCardView | undefined {
    return cards.value.get(id)
  }

  /** The visible cards of one seat's zone, in the zone's stored order. */
  function cardsIn(seatId: number, zone: PlayZone): PlayCardView[] {
    const seat = seatById(seatId)
    if (!seat) return []
    const ids = zone === 'hand' ? (seat.hand ?? []) : zone === 'library' ? [] : seat[zone]
    const out: PlayCardView[] = []
    for (const id of ids) {
      const c = cards.value.get(id)
      if (c) out.push(c)
    }
    return out
  }

  // ---- socket ----
  let socket: PlaySocket | null = null
  let factory: SocketFactory | undefined

  /** Tests inject a fake `WebSocket` here before `connect`. */
  function useSocketFactory(f: SocketFactory | undefined): void {
    factory = f
  }

  function handleMessage(message: PlayServerMessage): void {
    switch (message.type) {
      case 'lobby':
        room.value = message.room
        mySeatId.value = message.viewer_seat
        if (message.room.status === 'lobby') {
          // A room that went back to (or is still in) its lobby has no table.
          tableStatus.value = null
        }
        return
      case 'snapshot':
        applySnapshot(message.snapshot)
        return
      case 'patch':
        applyPatch(message.patch)
        return
      case 'peek':
        peek.value = message.peek
        return
      case 'error':
        lastError.value = { id: message.id, code: message.code, message: message.message }
        return
      case 'pong':
        return
      case 'closed':
        closedReason.value = message.reason
        return
    }
  }

  function applySnapshot(snapshot: PlaySnapshot): void {
    // Whatever gap we were waiting on, this is the answer to it.
    resyncPending = false
    version.value = snapshot.version
    tableStatus.value = snapshot.status
    format.value = snapshot.format
    startingLife.value = snapshot.starting_life
    mySeatId.value = snapshot.viewer_seat
    seats.value = snapshot.seats
    const next = new Map<number, PlayCardView>()
    for (const c of snapshot.cards) next.set(c.id, c)
    cards.value = next
    turn.value = snapshot.turn
    log.value = snapshot.log
    winner.value = snapshot.winner
    if (room.value) room.value = { ...room.value, status: snapshot.status }
  }

  function applyPatch(patch: PlayPatch): void {
    if (patch.version !== version.value + 1) {
      // One ask per gap (see `resyncPending`); the rest of the burst is dropped silently.
      if (!resyncPending && socket?.send({ type: 'resync' })) resyncPending = true
      return
    }
    version.value = patch.version
    tableStatus.value = patch.status
    turn.value = patch.turn
    winner.value = patch.winner
    if (patch.seats.length) {
      const byId = new Map(patch.seats.map((s) => [s.id, s]))
      seats.value = seats.value.map((s) => byId.get(s.id) ?? s)
    }
    if (patch.cards.length || patch.removed.length) {
      const map = cards.value
      for (const id of patch.removed) map.delete(id)
      for (const c of patch.cards) map.set(c.id, c)
      triggerRef(cards)
    }
    if (patch.log.length) {
      const merged = [...log.value, ...patch.log]
      log.value = merged.length > 300 ? merged.slice(merged.length - 300) : merged
    }
    if (room.value && room.value.status !== patch.status) {
      room.value = { ...room.value, status: patch.status }
    }
  }

  /** Open the socket for `code`, greeting with `seatToken` (`null` = watch only). */
  function connect(nextGame: string, nextCode: string, seatToken: string | null): void {
    if (game.value !== nextGame || code.value !== nextCode) reset()
    game.value = nextGame
    code.value = nextCode
    closedReason.value = null
    closedCode.value = null
    // A new socket is a new patch stream: whatever gap the old one had is moot, and the
    // `hello` this one sends answers with a snapshot regardless.
    resyncPending = false
    socket?.disconnect()
    socket = new PlaySocket(
      {
        onMessage: handleMessage,
        onConnection: (state) => {
          connection.value = state
        },
        onClosed: (code, reason) => {
          closedCode.value = code
          closedReason.value = reason || closedReason.value || 'connection closed'
        },
      },
      factory,
    )
    socket.connect(playSocketUrl(nextGame, nextCode), seatToken)
  }

  function disconnect(): void {
    socket?.disconnect()
    socket = null
    connection.value = 'idle'
  }

  /** Forget everything (leaving the room page). */
  function reset(): void {
    disconnect()
    game.value = ''
    code.value = ''
    mySeatId.value = null
    closedReason.value = null
    closedCode.value = null
    resyncPending = false
    room.value = null
    version.value = 0
    tableStatus.value = null
    format.value = ''
    startingLife.value = 0
    seats.value = []
    cards.value = new Map()
    turn.value = null
    log.value = []
    winner.value = null
    peek.value = null
    lastError.value = null
  }

  /** Send a table action. Returns the action id (echoed on a rejection), or null if offline. */
  function send(action: PlayAction): number | null {
    const id = nextActionId++
    return socket?.sendAction(action, id) ? id : null
  }

  /** Host only: ask the server to build the table from the lobby's decks. */
  function start(): boolean {
    return socket?.send({ type: 'start' }) ?? false
  }

  /**
   * Forget a close that has been dealt with — the session re-joining after a rotated token
   * (`4003`), which is a recovery rather than an ending and must not leave "this room is no
   * longer available" on screen behind the join card.
   */
  function clearClosed(): void {
    closedReason.value = null
    closedCode.value = null
  }

  function clearPeek(): void {
    peek.value = null
  }

  function clearError(): void {
    lastError.value = null
  }

  return {
    // identity
    game,
    code,
    connection,
    mySeatId,
    closedReason,
    closedCode,
    // lobby
    room,
    // table
    version,
    status,
    hasTable,
    format,
    startingLife,
    seats,
    cards,
    turn,
    log,
    winner,
    peek,
    lastError,
    mySeat,
    isHost,
    opponents,
    isMyTurn,
    seatById,
    card,
    cardsIn,
    // lifecycle
    useSocketFactory,
    connect,
    disconnect,
    reset,
    send,
    start,
    clearClosed,
    clearPeek,
    clearError,
    // exposed for tests
    handleMessage,
  }
})
