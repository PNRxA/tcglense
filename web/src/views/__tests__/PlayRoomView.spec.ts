import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { createPinia, setActivePinia } from 'pinia'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import PlayRoomView from '@/views/PlayRoomView.vue'
import { usePlayRoomStore } from '@/stores/playRoom'
import type { PlaySeatSnapshot, PlaySnapshot } from '@/lib/api/play'

// The room page is two pages in one, and the seam between them is where things go missing: once
// the table is up it covers the viewport, so anything the page still needs to say — or any
// event the table still needs answered — has to be wired *through* that cover. Both of the
// things pinned here were unreachable on the table side: the Leave button went nowhere, and
// "this room is no longer available" rendered underneath a full-screen table.

const session = vi.hoisted(() => ({}) as Record<string, unknown>)
const retry = vi.hoisted(() => vi.fn<() => void>())
const clearSeat = vi.hoisted(() => vi.fn<() => void>())
const leaveSeat = vi.hoisted(() => vi.fn<(vars: unknown) => Promise<unknown>>())
/** Every mutation the page reaches for but no test here exercises. */
const noop = vi.hoisted(() => vi.fn<(vars?: unknown) => Promise<unknown>>())
const deleteRoom = vi.hoisted(() => vi.fn<(vars: unknown) => Promise<unknown>>())

vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({ isAuthenticated: true, sessionResolved: true, user: { id: 1 } }),
}))
vi.mock('@/composables/useCatalog', async () => {
  const { ref } = await import('vue')
  return { useGameName: () => ref('Magic: The Gathering') }
})
vi.mock('@/composables/usePlayRooms', async () => {
  const { ref } = await import('vue')
  return {
    useLeavePlaySeat: () => ({ mutateAsync: leaveSeat, isPending: ref(false), error: ref(null) }),
    useDeletePlayRoom: () => ({ mutateAsync: deleteRoom, isPending: ref(false), error: ref(null) }),
    // The lobby half of the page reaches for these; nothing here exercises them.
    useSetPlaySeatReady: () => ({ mutateAsync: noop, isPending: ref(false), error: ref(null) }),
    useLoadPlaySeatDeck: () => ({ mutateAsync: noop, isPending: ref(false), error: ref(null) }),
  }
})
vi.mock('@/composables/usePlayRoomSession', async () => {
  const { ref, computed } = await import('vue')
  return {
    usePlayRoomSession: () => {
      Object.assign(session, {
        phase: ref('seated'),
        room: computed(() => ROOM),
        seat: ref(null),
        seatId: computed(() => 11),
        seatToken: ref('seat-token'),
        isLoading: computed(() => false),
        notFound: computed(() => false),
        error: ref(null),
        closedReason: ref(null),
        isJoining: ref(false),
        join: noop,
        watch: noop,
        clearSeat,
        retry,
      })
      return session
    },
  }
})

const ROOM = {
  id: 1,
  code: 'ABC234',
  game: 'mtg',
  label: 'Thursday pod',
  format: 'commander',
  starting_life: 40,
  max_players: 4,
  status: 'playing',
  viewer_seat: 11,
  seats: [],
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-01T00:00:00Z',
}

function seat(): PlaySeatSnapshot {
  return {
    id: 11,
    seat_index: 0,
    name: 'Ana',
    is_host: true,
    deck_name: null,
    life: 40,
    counters: {},
    commander_damage: {},
    out: false,
    connected: true,
    library_count: 92,
    hand_count: 0,
    hand: [],
    battlefield: [],
    graveyard: [],
    exile: [],
    command: [],
  }
}

function snapshot(): PlaySnapshot {
  return {
    version: 1,
    status: 'playing',
    format: 'commander',
    starting_life: 40,
    viewer_seat: 11,
    seats: [seat()],
    cards: [],
    turn: { number: 1, active_seat: 11, phase: 'main1' },
    log: [],
    winner: null,
  }
}

function makeRouter() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/tools', component: { template: '<div />' } },
      { path: '/tools/:game', component: { template: '<div />' } },
      { path: '/tools/:game/play', component: { template: '<div />' } },
      { path: '/tools/:game/play/:code', component: { template: '<div />' } },
    ],
  })
}

/** The page, optionally with the table already up (a snapshot is what makes it a table). */
async function mountRoom({ atTable = false } = {}) {
  // The page loads the table as its own chunk. Importing it here first means the view's
  // `defineAsyncComponent` gets an already-resolved module and mounts within a flush, rather
  // than a second later with the test long finished.
  await import('@/components/play/table/PlayTable.vue')
  const store = usePlayRoomStore()
  if (atTable) store.handleMessage({ type: 'snapshot', snapshot: snapshot() })
  const router = makeRouter()
  await router.push('/tools/mtg/play/ABC234')
  await router.isReady()
  const wrapper = mount(PlayRoomView, {
    props: { game: 'mtg', code: 'ABC234' },
    global: { plugins: [router, [VueQueryPlugin, { queryClient: new QueryClient() }]] },
  })
  await flushPromises()
  return { wrapper, router, store }
}

beforeEach(() => {
  setActivePinia(createPinia())
  retry.mockReset()
  clearSeat.mockReset()
  leaveSeat.mockReset().mockResolvedValue(undefined)
  deleteRoom.mockReset().mockResolvedValue(undefined)
})

describe('PlayRoomView at the table', () => {
  it("answers the table's Leave by going back to the hub, seat intact", async () => {
    const { wrapper, router, store } = await mountRoom({ atTable: true })
    expect(wrapper.find('[aria-label="Leave the table"]').exists()).toBe(true)

    await wrapper.find('[aria-label="Leave the table"]').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.path).toBe('/tools/mtg/play')
    // Walking away from the screen is not giving up the seat: no `DELETE .../seats/{id}` and
    // no forgotten token, so the invite link (or the hub's list) comes back to the same chair.
    expect(leaveSeat).not.toHaveBeenCalled()
    expect(clearSeat).not.toHaveBeenCalled()
    // The socket goes, though — nothing should keep streaming a table nobody is looking at.
    expect(store.connection).toBe('idle')
    expect(store.hasTable).toBe(false)
  })

  it('says the room is gone over the top of the table', async () => {
    const { wrapper } = await mountRoom({ atTable: true })
    ;(session.closedReason as { value: string | null }).value = 'the host closed this room'
    await flushPromises()

    // The host closing the room mid-game is exactly when this must be readable, and the table
    // is `fixed inset-0 z-50` — the message used to render in a branch the table replaced.
    expect(wrapper.text()).toContain('This room is no longer available')
    expect(wrapper.text()).toContain('the host closed this room')
    const notice = wrapper.find('.z-\\[60\\]')
    expect(notice.exists()).toBe(true)
    // And the one useful next step is a real link out, not a dead end.
    expect(notice.find('a').attributes('href')).toBe('/tools/mtg/play')
  })
})

describe('PlayRoomView before the table', () => {
  it('offers to try again when the seat could not be claimed', async () => {
    const { wrapper } = await mountRoom()
    ;(session.phase as { value: string }).value = 'retry'
    ;(session.error as { value: string | null }).value = 'service unavailable'
    await flushPromises()

    expect(wrapper.text()).toContain("Couldn't reach your seat")
    const again = wrapper.findAll('button').find((b) => b.text().includes('Try again'))
    await again?.trigger('click')

    // The token is still ours; the only correct verb is replaying it, never a fresh join.
    expect(retry).toHaveBeenCalledTimes(1)
  })

  it('shows the closed notice in the page when there is no table', async () => {
    const { wrapper } = await mountRoom()
    ;(session.closedReason as { value: string | null }).value = 'this room was deleted'
    await flushPromises()

    expect(wrapper.text()).toContain('This room is no longer available')
    // In the page it is a card, not an overlay — there is nothing to cover.
    expect(wrapper.find('.z-\\[60\\]').exists()).toBe(false)
  })
})
