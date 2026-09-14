import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  PlaySocket,
  RECONNECT_MIN_MS,
  type PlayConnection,
  type SocketLike,
} from '@/lib/playSocket'
import type { PlayServerMessage } from '@/lib/api/play'

// The socket is the one piece of the play table that has to survive the real world — a tab
// backgrounded on a phone, a train tunnel, a server restart — so what's pinned here is the
// behaviour a user would feel: it greets with its seat token (without which the server would
// seat them as a spectator), it comes back on its own after a drop, it does NOT come back when
// the server says the room is gone, and it never pretends to have sent something it dropped.

/** A hand-driven `WebSocket`: tests fire `open`/`message`/`close` themselves. */
class FakeSocket implements SocketLike {
  readyState = 0
  sent: string[] = []
  closedWith: Array<{ code?: number; reason?: string }> = []
  onopen: ((ev: unknown) => void) | null = null
  onmessage: ((ev: { data: unknown }) => void) | null = null
  onclose: ((ev: { code: number; reason: string }) => void) | null = null
  onerror: ((ev: unknown) => void) | null = null

  constructor(readonly url: string) {}

  send(data: string): void {
    this.sent.push(data)
  }

  close(code?: number, reason?: string): void {
    this.closedWith.push({ code, reason })
  }

  open(): void {
    this.readyState = 1
    this.onopen?.({})
  }

  message(payload: PlayServerMessage): void {
    this.onmessage?.({ data: JSON.stringify(payload) })
  }

  serverClose(code: number, reason = ''): void {
    this.readyState = 3
    this.onclose?.({ code, reason })
  }
}

function harness() {
  const sockets: FakeSocket[] = []
  const messages: PlayServerMessage[] = []
  const states: PlayConnection[] = []
  const closed: Array<{ code: number; reason: string }> = []
  const socket = new PlaySocket(
    {
      onMessage: (message) => messages.push(message),
      onConnection: (state) => states.push(state),
      onClosed: (code, reason) => closed.push({ code, reason }),
    },
    (url) => {
      const fake = new FakeSocket(url)
      sockets.push(fake)
      return fake
    },
  )
  const latest = () => sockets[sockets.length - 1]!
  return { socket, sockets, messages, states, closed, latest }
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('PlaySocket', () => {
  it('sends hello with the seat token as its first frame', () => {
    const { socket, latest, states } = harness()
    socket.connect('ws://x/ws', 'seat-token')
    expect(states).toEqual(['idle', 'connecting'])

    latest().open()

    expect(JSON.parse(latest().sent[0]!)).toEqual({ type: 'hello', seat_token: 'seat-token' })
    expect(states[states.length - 1]).toBe('open')
  })

  it('greets as a spectator when there is no token', () => {
    const { socket, latest } = harness()
    socket.connect('ws://x/ws', null)
    latest().open()
    // Explicitly null, not absent: the server reads "watch only" off the field.
    expect(JSON.parse(latest().sent[0]!)).toEqual({ type: 'hello', seat_token: null })
  })

  it('hands parsed frames to the listener and ignores unparseable ones', () => {
    const { socket, latest, messages } = harness()
    socket.connect('ws://x/ws', 't')
    latest().open()

    latest().message({ type: 'pong' })
    latest().onmessage?.({ data: 'not json' })

    expect(messages).toEqual([{ type: 'pong' }])
  })

  it('reconnects after an unclean close, backing off between attempts', () => {
    const { socket, sockets, latest, states } = harness()
    socket.connect('ws://x/ws', 't')
    latest().open()

    latest().serverClose(1006)
    expect(states[states.length - 1]).toBe('reconnecting')
    expect(sockets).toHaveLength(1)

    // First retry is the floor; nothing happens before it elapses.
    vi.advanceTimersByTime(RECONNECT_MIN_MS - 1)
    expect(sockets).toHaveLength(1)
    vi.advanceTimersByTime(1)
    expect(sockets).toHaveLength(2)

    // A second failure without an intervening hello waits twice as long.
    latest().serverClose(1006)
    vi.advanceTimersByTime(RECONNECT_MIN_MS)
    expect(sockets).toHaveLength(2)
    vi.advanceTimersByTime(RECONNECT_MIN_MS)
    expect(sockets).toHaveLength(3)

    // And every reconnect re-greets, so the seat is re-claimed rather than lost.
    latest().open()
    expect(JSON.parse(latest().sent[0]!)).toEqual({ type: 'hello', seat_token: 't' })
  })

  it('stays down after a final 4xxx close and reports the reason', () => {
    const { socket, sockets, latest, closed, states } = harness()
    socket.connect('ws://x/ws', 'stale')
    latest().open()

    // The server says why before closing; that reason is what the page shows.
    latest().message({ type: 'closed', reason: 'the host closed this room' })
    latest().serverClose(4004)

    vi.advanceTimersByTime(60_000)
    expect(sockets).toHaveLength(1)
    expect(states[states.length - 1]).toBe('closed')
    expect(closed).toEqual([{ code: 4004, reason: 'the host closed this room' }])
  })

  it('comes back from a 1011 — a server error is a hiccup, not an eviction', () => {
    const { socket, sockets, latest, closed, states } = harness()
    socket.connect('ws://x/ws', 'seat-token')
    latest().open()

    // 1011 is the server falling over mid-frame. It is outside the 4xxx range on purpose:
    // the seat is still ours and the room is still there, so treating it as final would
    // strand a whole pod on "this room is no longer available" after one restart.
    latest().serverClose(1011, 'internal error')

    expect(closed).toEqual([])
    expect(states[states.length - 1]).toBe('reconnecting')
    vi.advanceTimersByTime(RECONNECT_MIN_MS)
    expect(sockets).toHaveLength(2)
    latest().open()
    expect(JSON.parse(latest().sent[0]!)).toEqual({ type: 'hello', seat_token: 'seat-token' })
  })

  it('reports the close event reason when the server sent no closed frame', () => {
    const { socket, latest, closed } = harness()
    socket.connect('ws://x/ws', 'stale')
    latest().open()
    latest().serverClose(4003, 'bad seat token')
    expect(closed).toEqual([{ code: 4003, reason: 'bad seat token' }])
  })

  it('drops a send while not open and says so, rather than silently losing it', () => {
    const { socket, latest } = harness()
    socket.connect('ws://x/ws', 't')

    // Still connecting: nothing can go out yet.
    expect(socket.sendAction({ type: 'shuffle' })).toBe(false)
    expect(socket.isOpen).toBe(false)

    latest().open()
    expect(socket.isOpen).toBe(true)
    expect(socket.sendAction({ type: 'shuffle' }, 7)).toBe(true)
    expect(JSON.parse(latest().sent[1]!)).toEqual({
      type: 'action',
      id: 7,
      action: { type: 'shuffle' },
    })

    latest().serverClose(4004)
    expect(socket.sendAction({ type: 'shuffle' })).toBe(false)
  })

  it('does not reconnect after the client disconnects', () => {
    const { socket, sockets, latest, states } = harness()
    socket.connect('ws://x/ws', 't')
    latest().open()

    socket.disconnect()
    expect(latest().closedWith).toEqual([{ code: 1000, reason: 'client closed' }])
    expect(states[states.length - 1]).toBe('idle')

    vi.advanceTimersByTime(60_000)
    expect(sockets).toHaveLength(1)
  })
})
