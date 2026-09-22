import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, ref, type Ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import { ApiError } from '@/lib/api'
import type { PlayJoinResponse, PlayRoomSummary, PlaySeatView } from '@/lib/api/play'
import { usePlayRoomSession, type PlayRoomSession } from '@/composables/usePlayRoomSession'
import { rememberSeatToken, recallSeatToken } from '@/lib/playSeat'
import type { SocketLike } from '@/lib/playSocket'
import { usePlayRoomStore } from '@/stores/playRoom'

// Getting into a room is the feature's whole funnel, and each branch of it is a way to lose a
// player: a remembered token the server no longer likes, a signed-in host whose seat must come
// back to them, a guest who hasn't said their name yet. These pin the resolution order and,
// crucially, that a bad stored token *falls through* instead of becoming a dead end.

const { getPlayRoom, joinPlayRoom, auth } = vi.hoisted(() => ({
  getPlayRoom: vi.fn<(...args: unknown[]) => Promise<unknown>>(),
  joinPlayRoom: vi.fn<(...args: unknown[]) => Promise<unknown>>(),
  auth: { isAuthenticated: false, sessionResolved: true, user: null as unknown },
}))

vi.mock('@/lib/api/play', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/api/play')>()),
  getPlayRoom,
  joinPlayRoom,
}))
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({
    ...auth,
    authFetch: <T>(fn: (token: string) => Promise<T>) => fn('session-token'),
  }),
}))

const SEAT: PlaySeatView = {
  id: 11,
  seat_index: 0,
  display_name: 'Ada',
  is_host: true,
  is_user: true,
  ready: false,
  connected: true,
  deck_source: null,
  deck_name: null,
  deck_card_count: null,
  commanders: [],
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
  seats: [SEAT],
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-01T00:00:00Z',
}

function joinResponse(token: string, seat: PlaySeatView = SEAT): PlayJoinResponse {
  return { room: { ...ROOM, seats: [seat] }, seat, seat_token: token }
}

const mounted: Array<ReturnType<typeof mount>> = []
/** Every fake socket the store opened, newest last — how a test plays the server's goodbye. */
let sockets: SocketLike[] = []

/** Mount the composable with a fake socket, and hand the caller its state. */
function mountSession(code = 'ABC234'): PlayRoomSession & { codeRef: Ref<string> } {
  let session!: PlayRoomSession
  const codeRef = ref(code)
  const Host = defineComponent({
    setup() {
      const store = usePlayRoomStore()
      // No real WebSocket in jsdom; the store only needs something that looks like one.
      store.useSocketFactory(() => {
        const socket: SocketLike = {
          readyState: 1,
          send: () => {},
          close: () => {},
          onopen: null,
          onmessage: null,
          onclose: null,
          onerror: null,
        }
        sockets.push(socket)
        return socket
      })
      session = usePlayRoomSession(ref('mtg'), codeRef)
      return () => null
    },
  })
  mounted.push(
    mount(Host, {
      global: { plugins: [[VueQueryPlugin, { queryClient: new QueryClient() }]] },
    }),
  )
  return { ...session, codeRef }
}

/** The server hanging up on the current connection, the way `PlaySocket` hears it. */
function serverClose(code: number, reason = ''): void {
  sockets[sockets.length - 1]?.onclose?.({ code, reason })
}

beforeEach(() => {
  setActivePinia(createPinia())
  localStorage.clear()
  auth.isAuthenticated = false
  auth.sessionResolved = true
  getPlayRoom.mockReset().mockResolvedValue(ROOM)
  joinPlayRoom.mockReset()
  sockets = []
})

afterEach(() => {
  mounted.splice(0).forEach((wrapper) => wrapper.unmount())
})

describe('usePlayRoomSession', () => {
  it('replays a remembered seat token and connects with it', async () => {
    rememberSeatToken('mtg', 'ABC234', 'stored-token')
    joinPlayRoom.mockResolvedValue(joinResponse('stored-token'))

    const session = mountSession()
    await flushPromises()

    expect(joinPlayRoom).toHaveBeenCalledWith('mtg', 'ABC234', { seat_token: 'stored-token' })
    expect(session.phase.value).toBe('seated')
    expect(session.seat.value?.id).toBe(11)
    expect(session.seatToken.value).toBe('stored-token')
  })

  it('forgets a rejected token and falls through to a fresh join', async () => {
    rememberSeatToken('mtg', 'ABC234', 'stale-token')
    auth.isAuthenticated = true
    joinPlayRoom
      .mockRejectedValueOnce(new ApiError('invalid seat token', 401, 'invalid_seat_token'))
      .mockResolvedValueOnce(joinResponse('fresh-token'))

    const session = mountSession()
    await flushPromises()

    // A token the server won't take would fail the socket's hello too — it has to go.
    expect(joinPlayRoom).toHaveBeenCalledTimes(2)
    expect(joinPlayRoom.mock.calls[1]?.[2]).toEqual({})
    expect(session.phase.value).toBe('seated')
    expect(recallSeatToken('mtg', 'ABC234')).toBe('fresh-token')
  })

  it('keeps a token the server could not answer for, and offers to try again', async () => {
    rememberSeatToken('mtg', 'ABC234', 'my-token')
    joinPlayRoom.mockRejectedValue(new ApiError('service unavailable', 503))

    const session = mountSession()
    await flushPromises()

    // A 5xx (or a dead connection) says nothing about the seat. Falling through to a fresh
    // join would take a SECOND seat and leave the first one occupied by a ghost — the one
    // failure a player cannot undo themselves — so the token stays and `retry()` is the verb.
    expect(joinPlayRoom).toHaveBeenCalledTimes(1)
    expect(recallSeatToken('mtg', 'ABC234')).toBe('my-token')
    expect(session.phase.value).toBe('retry')
    expect(session.error.value).toBe('service unavailable')

    joinPlayRoom.mockResolvedValue(joinResponse('my-token'))
    session.retry()
    await flushPromises()

    expect(joinPlayRoom).toHaveBeenLastCalledWith('mtg', 'ABC234', { seat_token: 'my-token' })
    expect(session.phase.value).toBe('seated')
  })

  it('keeps the token when the join fails with no status at all (offline)', async () => {
    rememberSeatToken('mtg', 'ABC234', 'my-token')
    joinPlayRoom.mockRejectedValue(new TypeError('Failed to fetch'))

    const session = mountSession()
    await flushPromises()

    expect(recallSeatToken('mtg', 'ABC234')).toBe('my-token')
    expect(session.phase.value).toBe('retry')
  })

  it('forgets a token the server disowns with a 404 (the room was recreated)', async () => {
    rememberSeatToken('mtg', 'ABC234', 'stale-token')
    joinPlayRoom.mockRejectedValue(new ApiError('no such room', 404))

    const session = mountSession()
    await flushPromises()

    // Definitive: that token will never be taken again, and keeping it would fail the
    // socket's hello on every retry.
    expect(recallSeatToken('mtg', 'ABC234')).toBeNull()
    expect(session.phase.value).toBe('needs-name')
  })

  it('joins a signed-in visitor with no name — the server knows who they are', async () => {
    auth.isAuthenticated = true
    joinPlayRoom.mockResolvedValue(joinResponse('server-token'))

    const session = mountSession()
    await flushPromises()

    expect(joinPlayRoom).toHaveBeenCalledWith('mtg', 'ABC234', {}, 'session-token')
    expect(session.phase.value).toBe('seated')
  })

  it('asks a guest for a name instead of guessing one', async () => {
    const session = mountSession()
    await flushPromises()

    expect(joinPlayRoom).not.toHaveBeenCalled()
    expect(session.phase.value).toBe('needs-name')

    joinPlayRoom.mockResolvedValue(joinResponse('guest-token', { ...SEAT, display_name: 'Bo' }))
    await session.join('Bo')
    await flushPromises()

    expect(joinPlayRoom).toHaveBeenCalledWith('mtg', 'ABC234', { name: 'Bo' })
    expect(session.phase.value).toBe('seated')
    expect(recallSeatToken('mtg', 'ABC234')).toBe('guest-token')
  })

  it('leaves a guest on the join card when their name is refused', async () => {
    const session = mountSession()
    await flushPromises()

    joinPlayRoom.mockRejectedValue(new ApiError('that room is full', 409, 'room_full'))
    await session.join('Bo')
    await flushPromises()

    expect(session.error.value).toBe('that room is full')
    // Still on the form: a different name (or the watch button beside it) is one tap away.
    expect(session.phase.value).toBe('needs-name')
  })

  it('blocks a signed-in visitor whose join is refused, with the reason', async () => {
    auth.isAuthenticated = true
    joinPlayRoom.mockRejectedValue(new ApiError('this game has started', 409, 'room_started'))

    const session = mountSession()
    await flushPromises()

    expect(session.phase.value).toBe('blocked')
    expect(session.error.value).toBe('this game has started')
  })

  it('watches with no seat when asked', async () => {
    const session = mountSession()
    await flushPromises()

    session.watch()

    expect(session.phase.value).toBe('watching')
    expect(session.seatToken.value).toBeNull()
    expect(usePlayRoomStore().code).toBe('ABC234')
  })

  it('reports an unknown code rather than trying to seat anyone', async () => {
    getPlayRoom.mockRejectedValue(new ApiError('not found', 404))

    const session = mountSession('ZZZZZZ')
    await flushPromises()

    expect(session.notFound.value).toBe(true)
    expect(joinPlayRoom).not.toHaveBeenCalled()
  })

  it('waits for the session to resolve before deciding someone is a guest', async () => {
    auth.sessionResolved = false
    const session = mountSession()
    await flushPromises()

    // Deciding too early would mint a guest seat for a signed-in host reloading the page.
    expect(joinPlayRoom).not.toHaveBeenCalled()
    expect(session.phase.value).toBe('resolving')
  })

  it('clearSeat forgets the token and drops the connection', async () => {
    rememberSeatToken('mtg', 'ABC234', 'stored-token')
    joinPlayRoom.mockResolvedValue(joinResponse('stored-token'))
    const session = mountSession()
    await flushPromises()

    session.clearSeat()

    expect(recallSeatToken('mtg', 'ABC234')).toBeNull()
    expect(session.seat.value).toBeNull()
    expect(usePlayRoomStore().connection).toBe('idle')
  })

  it('re-joins a signed-in player whose seat token was rotated (4003)', async () => {
    auth.isAuthenticated = true
    rememberSeatToken('mtg', 'ABC234', 'laptop-token')
    joinPlayRoom
      .mockResolvedValueOnce(joinResponse('laptop-token'))
      .mockResolvedValueOnce(joinResponse('phone-token'))

    const session = mountSession()
    await flushPromises()
    expect(session.phase.value).toBe('seated')

    // Opening the same room on a phone re-issues the seat's token, and this connection's copy
    // stops naming a seat. The server hands the seat back by account, so the answer is to
    // join again — not to tell a player mid-game that their room is gone.
    serverClose(4003, 'that seat token is not for this room')
    await flushPromises()

    expect(joinPlayRoom).toHaveBeenCalledTimes(2)
    expect(joinPlayRoom.mock.calls[1]?.[2]).toEqual({})
    expect(session.phase.value).toBe('seated')
    expect(session.seatToken.value).toBe('phone-token')
    expect(recallSeatToken('mtg', 'ABC234')).toBe('phone-token')
    // And nothing anywhere says the room ended, because it didn't.
    expect(session.closedReason.value).toBeNull()
  })

  it('sends a guest whose token stopped working back to the join card', async () => {
    rememberSeatToken('mtg', 'ABC234', 'guest-token')
    joinPlayRoom.mockResolvedValue(joinResponse('guest-token'))

    const session = mountSession()
    await flushPromises()
    expect(session.phase.value).toBe('seated')

    serverClose(4003, 'that seat token is not for this room')
    await flushPromises()

    // A guest has no account to re-derive a seat from, so the honest answer is the form —
    // and a token the server won't take must not be replayed on the way there.
    expect(recallSeatToken('mtg', 'ABC234')).toBeNull()
    expect(session.phase.value).toBe('needs-name')
    expect(session.closedReason.value).toBeNull()
  })

  it('gives up rather than looping if the re-joined token is refused too', async () => {
    auth.isAuthenticated = true
    rememberSeatToken('mtg', 'ABC234', 'first-token')
    joinPlayRoom
      .mockResolvedValueOnce(joinResponse('first-token'))
      .mockResolvedValueOnce(joinResponse('second-token'))

    const session = mountSession()
    await flushPromises()
    serverClose(4003, 'that seat token is not for this room')
    await flushPromises()

    // Second refusal: something is wrong that another join won't fix, and a join loop is the
    // worst possible answer to a server saying no.
    serverClose(4003, 'that seat token is not for this room')
    await flushPromises()

    expect(joinPlayRoom).toHaveBeenCalledTimes(2)
    expect(session.closedReason.value).toBe('that seat token is not for this room')
  })

  it('leaves a 4004 close as the ending it is', async () => {
    auth.isAuthenticated = true
    rememberSeatToken('mtg', 'ABC234', 'my-token')
    joinPlayRoom.mockResolvedValue(joinResponse('my-token'))

    const session = mountSession()
    await flushPromises()

    serverClose(4004, 'the host closed this room')
    await flushPromises()

    // The room is gone: no re-join, and the reason stands so the page can say so.
    expect(joinPlayRoom).toHaveBeenCalledTimes(1)
    expect(session.closedReason.value).toBe('the host closed this room')
  })

  it('prefers the lobby the socket pushed over the polled summary', async () => {
    rememberSeatToken('mtg', 'ABC234', 'stored-token')
    joinPlayRoom.mockResolvedValue(joinResponse('stored-token'))
    const session = mountSession()
    await flushPromises()

    const store = usePlayRoomStore()
    store.handleMessage({
      type: 'lobby',
      room: { ...ROOM, seats: [{ ...SEAT, ready: true, deck_name: 'Atraxa' }] },
      viewer_seat: 11,
    })
    await flushPromises()

    expect(session.room.value?.seats[0]?.deck_name).toBe('Atraxa')
    // ...and the seat view tracks it, so the lobby's ready/deck state is never one write stale.
    expect(session.seat.value?.ready).toBe(true)
  })
})
