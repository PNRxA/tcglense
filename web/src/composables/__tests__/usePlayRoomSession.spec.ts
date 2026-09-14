import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, ref, type Ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import { ApiError } from '@/lib/api'
import type { PlayJoinResponse, PlayRoomSummary, PlaySeatView } from '@/lib/api/play'
import { usePlayRoomSession, type PlayRoomSession } from '@/composables/usePlayRoomSession'
import { rememberSeatToken, recallSeatToken } from '@/lib/playSeat'
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
  seats: [SEAT],
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-01T00:00:00Z',
}

function joinResponse(token: string, seat: PlaySeatView = SEAT): PlayJoinResponse {
  return { room: { ...ROOM, seats: [seat] }, seat, seat_token: token }
}

const mounted: Array<ReturnType<typeof mount>> = []

/** Mount the composable with a fake socket, and hand the caller its state. */
function mountSession(code = 'ABC234'): PlayRoomSession & { codeRef: Ref<string> } {
  let session!: PlayRoomSession
  const codeRef = ref(code)
  const Host = defineComponent({
    setup() {
      const store = usePlayRoomStore()
      // No real WebSocket in jsdom; the store only needs something that looks like one.
      store.useSocketFactory(() => ({
        readyState: 1,
        send: () => {},
        close: () => {},
        onopen: null,
        onmessage: null,
        onclose: null,
        onerror: null,
      }))
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

beforeEach(() => {
  setActivePinia(createPinia())
  localStorage.clear()
  auth.isAuthenticated = false
  auth.sessionResolved = true
  getPlayRoom.mockReset().mockResolvedValue(ROOM)
  joinPlayRoom.mockReset()
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
