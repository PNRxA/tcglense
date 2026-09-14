import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import PlayHubView from '@/views/PlayHubView.vue'
import type { PlayRoomSummary } from '@/lib/api/play'

// The hub has to work for two people at once: the host, who needs an account, and the friend
// they sent a code to, who does not. The rule that keeps the feature usable is that being
// signed out costs you the *left* column only — a code box behind a login wall would turn every
// invite read out over voice chat into a dead end, which is the regression this pins.

const auth = vi.hoisted(() => ({
  isAuthenticated: true,
  sessionResolved: true,
  user: { id: 1, username: 'ada' } as { id: number; username: string | null } | null,
}))
const state = vi.hoisted(() => ({
  rooms: [] as PlayRoomSummary[],
  isPending: false,
  isLoadingError: false,
  isRefetchError: false,
}))
const createRoom = vi.hoisted(() => vi.fn<(vars: unknown) => Promise<unknown>>())
const deleteRoom = vi.hoisted(() => vi.fn<(vars: unknown) => Promise<unknown>>())

vi.mock('@/stores/auth', () => ({ useAuthStore: () => auth }))
vi.mock('@/composables/useCatalog', async () => {
  const { ref } = await import('vue')
  return { useGameName: () => ref('Magic: The Gathering') }
})
vi.mock('@/composables/usePlayRooms', async () => {
  const { ref } = await import('vue')
  return {
    usePlayRoomsQuery: () => ({
      data: ref({ data: state.rooms }),
      isPending: ref(state.isPending),
      isLoadingError: ref(state.isLoadingError),
      isRefetchError: ref(state.isRefetchError),
    }),
    useCreatePlayRoom: () => ({
      mutateAsync: createRoom,
      isPending: ref(false),
      error: ref(null),
    }),
    useDeletePlayRoom: () => ({
      mutateAsync: deleteRoom,
      isPending: ref(false),
      error: ref(null),
    }),
  }
})

function room(over: Partial<PlayRoomSummary> = {}): PlayRoomSummary {
  return {
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
    ...over,
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
      { path: '/login', component: { template: '<div />' } },
      { path: '/register', component: { template: '<div />' } },
    ],
  })
}

async function mountHub() {
  const router = makeRouter()
  await router.push('/tools/mtg/play')
  await router.isReady()
  const wrapper = mount(PlayHubView, {
    props: { game: 'mtg' },
    global: { plugins: [router] },
  })
  await flushPromises()
  return { wrapper, router }
}

function codeInput(wrapper: Awaited<ReturnType<typeof mountHub>>['wrapper']) {
  return wrapper.find('#play-join-code')
}

beforeEach(() => {
  auth.isAuthenticated = true
  auth.sessionResolved = true
  auth.user = { id: 1, username: 'ada' }
  state.rooms = []
  state.isPending = false
  state.isLoadingError = false
  state.isRefetchError = false
  createRoom.mockReset().mockResolvedValue({ room: room({ code: 'NEW123' }), seat_token: 'tok' })
  deleteRoom.mockReset().mockResolvedValue(undefined)
  localStorage.clear()
})

describe('PlayHubView signed out', () => {
  it('keeps join-by-code and replaces only the host half with a sign-in prompt', async () => {
    auth.isAuthenticated = false
    const { wrapper } = await mountHub()

    expect(wrapper.findComponent({ name: 'PlaySignInPrompt' }).exists()).toBe(true)
    expect(wrapper.findComponent({ name: 'PlayCreateRoomForm' }).exists()).toBe(false)
    expect(wrapper.text()).not.toContain('Your rooms')
    // The guest path survives: this is the whole reason the page isn't `requiresAuth`.
    expect(codeInput(wrapper).exists()).toBe(true)
  })

  it('opens the room route for a six-character code, and not before', async () => {
    auth.isAuthenticated = false
    const { wrapper, router } = await mountHub()
    const submit = wrapper.findAll('button').find((b) => b.text().includes('Join'))

    expect(submit?.attributes('disabled')).toBeDefined()

    await codeInput(wrapper).setValue('abc234')
    expect(submit?.attributes('disabled')).toBeUndefined()
    // Submitting the form, not clicking the button: jsdom doesn't implicitly submit, and the
    // Enter key a player actually presses after typing a code goes through the same handler.
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    // Lower case in, upper case out: the code is case-insensitive and the route is canonical.
    expect(router.currentRoute.value.fullPath).toBe('/tools/mtg/play/ABC234')
  })
})

describe('PlayHubView signed in', () => {
  it('shows the create form and the room list', async () => {
    state.rooms = [room()]
    const { wrapper } = await mountHub()

    expect(wrapper.findComponent({ name: 'PlayCreateRoomForm' }).exists()).toBe(true)
    expect(wrapper.findComponent({ name: 'PlaySignInPrompt' }).exists()).toBe(false)
    expect(wrapper.text()).toContain('Thursday pod')
    expect(wrapper.text()).toContain('ABC234')
  })

  it('puts a game in progress above the lobbies, and finished ones last', async () => {
    state.rooms = [
      room({ id: 1, code: 'FIN111', status: 'finished', label: 'Done' }),
      room({ id: 2, code: 'LOB222', status: 'lobby', label: 'Waiting' }),
      room({ id: 3, code: 'PLA333', status: 'playing', label: 'Running' }),
    ]
    const { wrapper } = await mountHub()

    const labels = wrapper
      .findAllComponents({ name: 'PlayRoomRow' })
      .map((row) => (row.props('room') as PlayRoomSummary).label)
    expect(labels).toEqual(['Running', 'Waiting', 'Done'])
  })

  it("offers the close action only on rooms whose host seat is the caller's own", async () => {
    const host = {
      id: 5,
      seat_index: 0,
      display_name: 'ada',
      is_host: true,
      is_user: true,
      ready: false,
      connected: false,
      deck_source: null,
      deck_name: null,
      deck_card_count: null,
      commanders: [],
    }
    state.rooms = [
      room({ id: 1, code: 'MINE11', label: 'Mine', seats: [host], viewer_seat: 5 }),
      room({ id: 2, code: 'THEIR2', label: 'Theirs', seats: [host], viewer_seat: 9 }),
      room({ id: 3, code: 'NONE33', label: 'Stranger', seats: [host], viewer_seat: null }),
    ]
    const { wrapper } = await mountHub()

    const closable = wrapper
      .findAllComponents({ name: 'PlayRoomRow' })
      .filter((row) => row.find('button[aria-label^="Close room"]').exists())
      .map((row) => (row.props('room') as PlayRoomSummary).label)
    expect(closable).toEqual(['Mine'])
  })

  it('remembers the host seat token and navigates to the new room', async () => {
    const { wrapper, router } = await mountHub()
    await wrapper.findComponent({ name: 'PlayCreateRoomForm' }).vm.$emit('create', {
      format: 'commander',
      starting_life: 40,
      max_players: 4,
    })
    await flushPromises()

    expect(createRoom).toHaveBeenCalledWith({
      game: 'mtg',
      body: { format: 'commander', starting_life: 40, max_players: 4 },
    })
    // Stored before the navigation: a refresh on the way in must not cost the host their seat.
    expect(localStorage.getItem('tcglense_play_seat:mtg:NEW123')).toBe('tok')
    expect(router.currentRoute.value.fullPath).toBe('/tools/mtg/play/NEW123')
  })

  it('says the list is empty rather than showing a bare heading', async () => {
    const { wrapper } = await mountHub()
    expect(wrapper.text()).toContain('No rooms yet')
  })
})
