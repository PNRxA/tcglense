import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type {
  PlayCardView,
  PlayPatch,
  PlayRoomSummary,
  PlaySeatSnapshot,
  PlaySnapshot,
} from '@/lib/api/play'
import type { SocketLike } from '@/lib/playSocket'
import { usePlayRoomStore } from '@/stores/playRoom'

// The store is where "what the server said" becomes "what's on screen", and the failure modes
// are all silent ones: a dropped frame leaving the table one action behind forever, a token
// that left the battlefield still drawn, an opponent seated in the wrong chair. Each is pinned
// here against the patch stream the server actually sends.

class FakeSocket implements SocketLike {
  readyState = 1
  sent: string[] = []
  onopen: ((ev: unknown) => void) | null = null
  onmessage: ((ev: { data: unknown }) => void) | null = null
  onclose: ((ev: { code: number; reason: string }) => void) | null = null
  onerror: ((ev: unknown) => void) | null = null

  send(data: string): void {
    this.sent.push(data)
  }

  close(): void {
    this.readyState = 3
  }

  frames(): unknown[] {
    return this.sent.map((raw) => JSON.parse(raw))
  }
}

function seat(id: number, index: number, over: Partial<PlaySeatSnapshot> = {}): PlaySeatSnapshot {
  return {
    id,
    seat_index: index,
    name: `Seat ${index}`,
    is_host: index === 0,
    deck_name: 'A deck',
    life: 40,
    counters: {},
    commander_damage: {},
    out: false,
    connected: true,
    library_count: 92,
    hand_count: 7,
    hand: null,
    battlefield: [],
    graveyard: [],
    exile: [],
    command: [],
    ...over,
  }
}

function card(id: number, over: Partial<PlayCardView> = {}): PlayCardView {
  return {
    id,
    def: null,
    owner: 1,
    controller: 1,
    zone: 'battlefield',
    tapped: false,
    face_down: false,
    face_index: 0,
    revealed: false,
    counters: {},
    x: 0,
    y: 0,
    attached_to: null,
    power_toughness: null,
    ...over,
  }
}

const TURN = { number: 1, active_seat: 1, phase: 'main1' as const }

function snapshot(over: Partial<PlaySnapshot> = {}): PlaySnapshot {
  return {
    version: 5,
    status: 'playing',
    format: 'commander',
    starting_life: 40,
    viewer_seat: 2,
    seats: [seat(1, 0), seat(2, 1), seat(3, 2)],
    cards: [card(10), card(11)],
    turn: TURN,
    log: [],
    winner: null,
    ...over,
  }
}

function patch(over: Partial<PlayPatch> = {}): PlayPatch {
  return {
    version: 6,
    status: 'playing',
    seats: [],
    cards: [],
    removed: [],
    turn: TURN,
    log: [],
    winner: null,
    ...over,
  }
}

const ROOM: PlayRoomSummary = {
  id: 1,
  code: 'ABC234',
  game: 'mtg',
  label: 'Thursday pod',
  format: 'commander',
  starting_life: 40,
  max_players: 4,
  status: 'lobby',
  viewer_seat: null,
  seats: [],
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-01T00:00:00Z',
}

let socket: FakeSocket

/** A connected store with a hand-driven socket, so `resync` can be observed going out. */
function connectedStore() {
  const store = usePlayRoomStore()
  socket = new FakeSocket()
  store.useSocketFactory(() => socket)
  store.connect('mtg', 'ABC234', 'seat-token')
  return store
}

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('lobby frames', () => {
  it('adopts the pushed room and the seat it says is ours', () => {
    const store = connectedStore()
    store.handleMessage({ type: 'lobby', room: ROOM, viewer_seat: 7 })

    expect(store.room?.code).toBe('ABC234')
    expect(store.mySeatId).toBe(7)
    expect(store.status).toBe('lobby')
    // No table until a snapshot arrives — the room page must keep showing the lobby.
    expect(store.hasTable).toBe(false)
  })

  it('reads is_host off the lobby seat before there is a table', () => {
    const store = connectedStore()
    store.handleMessage({
      type: 'lobby',
      room: {
        ...ROOM,
        seats: [
          {
            id: 7,
            seat_index: 0,
            display_name: 'Host',
            is_host: true,
            is_user: true,
            ready: false,
            connected: true,
            deck_source: null,
            deck_name: null,
            deck_card_count: null,
            commanders: [],
          },
        ],
      },
      viewer_seat: 7,
    })
    expect(store.isHost).toBe(true)
  })
})

describe('snapshot then patches', () => {
  it('adopts a snapshot wholesale', () => {
    const store = connectedStore()
    store.handleMessage({ type: 'snapshot', snapshot: snapshot() })

    expect(store.hasTable).toBe(true)
    expect(store.version).toBe(5)
    expect(store.mySeatId).toBe(2)
    expect(store.seats).toHaveLength(3)
    expect(store.cards.size).toBe(2)
  })

  it('folds a consecutive patch in: changed seats, new cards, appended log', () => {
    const store = connectedStore()
    store.handleMessage({ type: 'snapshot', snapshot: snapshot() })

    store.handleMessage({
      type: 'patch',
      patch: patch({
        seats: [seat(2, 1, { life: 37 })],
        cards: [card(12, { tapped: true })],
        log: [
          {
            id: 1,
            seat: 2,
            kind: 'action',
            text: 'Seat 1 lost 3 life',
            at: '2026-09-01T00:00:00Z',
          },
        ],
      }),
    })

    expect(store.version).toBe(6)
    expect(store.seatById(2)?.life).toBe(37)
    // An untouched seat keeps its own row rather than being dropped by the partial list.
    expect(store.seatById(1)?.life).toBe(40)
    expect(store.card(12)?.tapped).toBe(true)
    expect(store.log).toHaveLength(1)
    expect(socket.frames()).not.toContainEqual({ type: 'resync' })
  })

  it('drops cards the patch removed', () => {
    const store = connectedStore()
    store.handleMessage({ type: 'snapshot', snapshot: snapshot() })

    // A token leaving the battlefield ceases to exist; leaving it drawn is the bug.
    store.handleMessage({ type: 'patch', patch: patch({ removed: [10] }) })

    expect(store.card(10)).toBeUndefined()
    expect(store.cards.size).toBe(1)
  })

  it('asks for a resync — and applies nothing — when a version is missed', () => {
    const store = connectedStore()
    store.handleMessage({ type: 'snapshot', snapshot: snapshot() })

    store.handleMessage({ type: 'patch', patch: patch({ version: 8, removed: [10] }) })

    expect(socket.frames()).toContainEqual({ type: 'resync' })
    // Guessing at the gap would show a table that never existed, so nothing moves.
    expect(store.version).toBe(5)
    expect(store.card(10)).toBeDefined()
  })

  it('asks once per gap, however many out-of-order patches arrive', () => {
    const store = connectedStore()
    store.handleMessage({ type: 'snapshot', snapshot: snapshot() })

    // A tab that was throttled in the background wakes to the whole burst it missed, and
    // every frame in it is out of order. One resync answers all of them; one *per patch* is
    // a flood the server answers with `too_fast` and then a 4008 close.
    for (const version of [8, 9, 10, 11]) {
      store.handleMessage({ type: 'patch', patch: patch({ version }) })
    }

    expect(socket.frames().filter((f) => JSON.stringify(f) === '{"type":"resync"}')).toHaveLength(
      1,
    )
    expect(store.version).toBe(5)
  })

  it('asks again for the next gap once the snapshot has landed', () => {
    const store = connectedStore()
    store.handleMessage({ type: 'snapshot', snapshot: snapshot() })
    store.handleMessage({ type: 'patch', patch: patch({ version: 8 }) })
    // The snapshot is the answer, so the next gap is a new question — the latch must not
    // leave the table stuck one dropped frame behind forever.
    store.handleMessage({ type: 'snapshot', snapshot: snapshot({ version: 8 }) })

    store.handleMessage({ type: 'patch', patch: patch({ version: 12 }) })

    expect(socket.frames().filter((f) => JSON.stringify(f) === '{"type":"resync"}')).toHaveLength(
      2,
    )
  })

  it('recovers from the gap when the fresh snapshot lands', () => {
    const store = connectedStore()
    store.handleMessage({ type: 'snapshot', snapshot: snapshot() })
    store.handleMessage({ type: 'patch', patch: patch({ version: 8 }) })

    store.handleMessage({
      type: 'snapshot',
      snapshot: snapshot({ version: 8, cards: [card(10)] }),
    })

    expect(store.version).toBe(8)
    expect(store.cards.size).toBe(1)
  })
})

describe('reading the table', () => {
  it('lists a zone in its stored order and skips cards it cannot see', () => {
    const store = connectedStore()
    store.handleMessage({
      type: 'snapshot',
      snapshot: snapshot({
        seats: [seat(1, 0, { battlefield: [11, 10], graveyard: [99] }), seat(2, 1)],
        cards: [card(10), card(11)],
      }),
    })

    expect(store.cardsIn(1, 'battlefield').map((c) => c.id)).toEqual([11, 10])
    // 99 is in the seat's graveyard list but not in our card map — a hidden card is skipped,
    // never rendered as a blank.
    expect(store.cardsIn(1, 'graveyard')).toEqual([])
    // A library is a count, never a list of ids: asking for one must not leak an order.
    expect(store.cardsIn(1, 'library')).toEqual([])
  })

  it('orders opponents clockwise from my seat', () => {
    const store = connectedStore()
    store.handleMessage({ type: 'snapshot', snapshot: snapshot({ viewer_seat: 2 }) })

    // Seats are [1, 2, 3] and mine is 2, so the table reads 3 then 1 — the order you'd see
    // going round from your own chair, which is what the board layout draws.
    expect(store.opponents.map((s) => s.id)).toEqual([3, 1])
    expect(store.isMyTurn).toBe(false)
  })

  it('gives a spectator every seat, in table order', () => {
    const store = connectedStore()
    store.handleMessage({ type: 'snapshot', snapshot: snapshot({ viewer_seat: null }) })

    expect(store.mySeatId).toBeNull()
    expect(store.opponents.map((s) => s.id)).toEqual([1, 2, 3])
  })
})

describe('errors, peeks and teardown', () => {
  it('keeps the last error and the last peek until the UI clears them', () => {
    const store = connectedStore()
    store.handleMessage({ type: 'error', id: 4, code: 'not_your_card', message: 'Not yours' })
    expect(store.lastError).toEqual({ id: 4, code: 'not_your_card', message: 'Not yours' })
    store.clearError()
    expect(store.lastError).toBeNull()

    store.handleMessage({
      type: 'peek',
      peek: { kind: 'look_top', cards: [] },
    })
    expect(store.peek).not.toBeNull()
    store.clearPeek()
    expect(store.peek).toBeNull()
  })

  it('remembers why the server closed us', () => {
    const store = connectedStore()
    store.handleMessage({ type: 'closed', reason: 'the host closed this room' })
    expect(store.closedReason).toBe('the host closed this room')
  })

  it('keeps the close code beside the reason, and lets a recovery clear both', () => {
    const store = connectedStore()
    // The reason is prose for the player; only the code says whether this is recoverable.
    socket.onclose?.({ code: 4003, reason: 'that seat token is not for this room' })

    expect(store.closedCode).toBe(4003)
    expect(store.closedReason).toBe('that seat token is not for this room')

    store.clearClosed()
    expect(store.closedCode).toBeNull()
    expect(store.closedReason).toBeNull()
  })

  it('reset drops the table so the next room starts clean', () => {
    const store = connectedStore()
    store.handleMessage({ type: 'snapshot', snapshot: snapshot() })

    store.reset()

    expect(store.hasTable).toBe(false)
    expect(store.seats).toEqual([])
    expect(store.cards.size).toBe(0)
    expect(store.mySeatId).toBeNull()
    expect(store.connection).toBe('idle')
  })
})
