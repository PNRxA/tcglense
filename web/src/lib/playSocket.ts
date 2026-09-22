import type { PlayAction, PlayClientMessage, PlayServerMessage } from '@/lib/api/play'

/**
 * The room socket, with none of the table's state in it: connect, say hello, hand every
 * server frame to a listener, and come back on our own after a drop. `stores/playRoom.ts`
 * owns what the frames *mean*; this class owns the wire so it can be unit-tested with a
 * fake `WebSocket` and the store with a fake socket.
 *
 * Reconnect policy: exponential backoff 1s → 10s (capped), reset on a clean hello. A
 * server-initiated close in the `4xxx` range (bad token, room gone, seat removed) is
 * final — it will not reconnect, and reports the reason through `onClosed`. Every
 * reconnect re-sends `hello`, and the store answers the fresh `snapshot`/`lobby` that
 * follows, so nothing here tracks versions.
 */

export type PlayConnection = 'idle' | 'connecting' | 'open' | 'reconnecting' | 'closed'

export interface PlaySocketHandlers {
  onMessage: (message: PlayServerMessage) => void
  onConnection: (state: PlayConnection) => void
  /** A final close (no reconnect): the code and the server's `closed.reason` when it sent one. */
  onClosed: (code: number, reason: string) => void
}

/** The subset of `WebSocket` the client uses — what a test fakes. */
export interface SocketLike {
  readyState: number
  send(data: string): void
  close(code?: number, reason?: string): void
  onopen: ((ev: unknown) => void) | null
  onmessage: ((ev: { data: unknown }) => void) | null
  onclose: ((ev: { code: number; reason: string }) => void) | null
  onerror: ((ev: unknown) => void) | null
}

export type SocketFactory = (url: string) => SocketLike

const SOCKET_OPEN = 1
export const RECONNECT_MIN_MS = 1_000
export const RECONNECT_MAX_MS = 10_000
export const PING_INTERVAL_MS = 25_000
/** Close codes the server uses for a final, don't-come-back close. */
export const FINAL_CLOSE_MIN = 4000
export const FINAL_CLOSE_MAX = 4999

export class PlaySocket {
  private socket: SocketLike | null = null
  private url = ''
  private seatToken: string | null = null
  private attempt = 0
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private pingTimer: ReturnType<typeof setInterval> | null = null
  private stopped = true
  private lastReason = ''

  constructor(
    private readonly handlers: PlaySocketHandlers,
    private readonly factory: SocketFactory = (url) => new WebSocket(url) as unknown as SocketLike,
  ) {}

  /** Open (or re-open) against `url`, greeting with `seatToken` (`null` = spectator). */
  connect(url: string, seatToken: string | null): void {
    this.disconnect()
    this.url = url
    this.seatToken = seatToken
    this.stopped = false
    this.attempt = 0
    this.open('connecting')
  }

  /** Close for good (no reconnect). Safe to call repeatedly. */
  disconnect(): void {
    this.stopped = true
    this.clearTimers()
    const socket = this.socket
    this.socket = null
    if (socket) {
      socket.onopen = socket.onmessage = socket.onclose = socket.onerror = null
      try {
        socket.close(1000, 'client closed')
      } catch {
        // Already closed.
      }
    }
    this.handlers.onConnection('idle')
  }

  get isOpen(): boolean {
    return this.socket?.readyState === SOCKET_OPEN
  }

  /** Send a table action. Returns false (and drops it) when not connected. */
  sendAction(action: PlayAction, id?: number): boolean {
    return this.send({ type: 'action', id: id ?? null, action })
  }

  send(message: PlayClientMessage): boolean {
    if (!this.socket || this.socket.readyState !== SOCKET_OPEN) return false
    this.socket.send(JSON.stringify(message))
    return true
  }

  private open(phase: 'connecting' | 'reconnecting'): void {
    this.handlers.onConnection(phase)
    let socket: SocketLike
    try {
      socket = this.factory(this.url)
    } catch {
      this.scheduleReconnect()
      return
    }
    this.socket = socket
    socket.onopen = () => {
      if (this.socket !== socket) return
      this.attempt = 0
      socket.send(JSON.stringify({ type: 'hello', seat_token: this.seatToken }))
      this.handlers.onConnection('open')
      this.startPing()
    }
    socket.onmessage = (ev) => {
      if (this.socket !== socket) return
      let parsed: PlayServerMessage
      try {
        parsed = JSON.parse(String(ev.data)) as PlayServerMessage
      } catch {
        return
      }
      if (parsed.type === 'closed') this.lastReason = parsed.reason
      this.handlers.onMessage(parsed)
    }
    socket.onerror = () => {
      // The close that follows carries the outcome; nothing to do here.
    }
    socket.onclose = (ev) => {
      if (this.socket !== socket) return
      this.socket = null
      this.clearTimers()
      if (this.stopped) return
      if (ev.code >= FINAL_CLOSE_MIN && ev.code <= FINAL_CLOSE_MAX) {
        this.stopped = true
        this.handlers.onConnection('closed')
        this.handlers.onClosed(ev.code, this.lastReason || ev.reason)
        return
      }
      this.scheduleReconnect()
    }
  }

  private scheduleReconnect(): void {
    if (this.stopped) return
    const delay = Math.min(RECONNECT_MAX_MS, RECONNECT_MIN_MS * 2 ** this.attempt)
    this.attempt += 1
    this.handlers.onConnection('reconnecting')
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null
      if (!this.stopped) this.open('reconnecting')
    }, delay)
  }

  private startPing(): void {
    this.pingTimer = setInterval(() => this.send({ type: 'ping' }), PING_INTERVAL_MS)
  }

  private clearTimers(): void {
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer)
    if (this.pingTimer) clearInterval(this.pingTimer)
    this.reconnectTimer = null
    this.pingTimer = null
  }
}
