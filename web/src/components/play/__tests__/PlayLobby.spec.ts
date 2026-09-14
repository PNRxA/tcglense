import { describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import PlayLobby from '../PlayLobby.vue'
import type { PlayRoomSummary, PlaySeatView } from '@/lib/api/play'

// The lobby's one job the server can't do for it: tell the host *why* they can't start yet.
// The two conditions (a second player, a deck in every seat) are the server's rules, so what's
// pinned here is that the button and the explanation agree with each other and with them — a
// disabled button with no reason beside it is the state this component exists to prevent.

// The ready toggle and the deck picker each fetch; stub them so this spec is about gating.
vi.mock('@/composables/usePlayRooms', async () => {
  const { ref } = await import('vue')
  return {
    useSetPlaySeatReady: () => ({
      mutateAsync: vi.fn<() => Promise<unknown>>(async () => ({})),
      isPending: ref(false),
      error: ref(null),
    }),
    useLoadPlaySeatDeck: () => ({
      mutateAsync: vi.fn<() => Promise<unknown>>(async () => ({})),
      isPending: ref(false),
      error: ref(null),
    }),
  }
})
vi.mock('@/composables/useDecks', async () => {
  const { ref } = await import('vue')
  return { useDecksQuery: () => ({ data: ref({ data: [] }), isPending: ref(false) }) }
})
vi.mock('@/composables/usePrecons', async () => {
  const { ref } = await import('vue')
  return { usePreconsQuery: () => ({ data: ref({ data: [] }), isPending: ref(false) }) }
})
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ isAuthenticated: true }) }))

function seat(id: number, over: Partial<PlaySeatView> = {}): PlaySeatView {
  return {
    id,
    seat_index: id - 1,
    display_name: `Player ${id}`,
    is_host: id === 1,
    is_user: true,
    ready: false,
    connected: true,
    deck_source: 'deck',
    deck_name: 'Atraxa',
    deck_card_count: 100,
    commanders: ['Atraxa, Praetors’ Voice'],
    ...over,
  }
}

function room(seats: PlaySeatView[], over: Partial<PlayRoomSummary> = {}): PlayRoomSummary {
  return {
    id: 1,
    code: 'ABC234',
    game: 'mtg',
    label: 'Thursday pod',
    format: 'commander',
    starting_life: 40,
    max_players: 4,
    status: 'lobby',
    seats,
    created_at: '2026-09-01T00:00:00Z',
    updated_at: '2026-09-01T00:00:00Z',
    ...over,
  }
}

function mountLobby(seats: PlaySeatView[], mySeat: PlaySeatView | null = seats[0] ?? null) {
  return mount(PlayLobby, {
    props: {
      game: 'mtg',
      code: 'ABC234',
      room: room(seats),
      mySeat,
      seatToken: 'seat-token',
    },
    global: { stubs: { RouterLink: true } },
  })
}

function startButton(wrapper: ReturnType<typeof mountLobby>) {
  return wrapper.findAll('button').find((b) => b.text().includes('Start the game'))
}

const blocker = (wrapper: ReturnType<typeof mountLobby>) =>
  wrapper.find('[data-testid="play-start-blocker"]')

describe('PlayLobby start gating', () => {
  it('enables Start once there are two seats and every one has a deck', () => {
    const wrapper = mountLobby([seat(1), seat(2)])
    expect(startButton(wrapper)?.attributes('disabled')).toBeUndefined()
    expect(blocker(wrapper).exists()).toBe(false)
  })

  it('waits for a second player, and says so', () => {
    const wrapper = mountLobby([seat(1)])
    expect(startButton(wrapper)?.attributes('disabled')).toBeDefined()
    expect(blocker(wrapper).text()).toContain('at least one more player')
  })

  it('names the players still without a deck', () => {
    const wrapper = mountLobby([seat(1), seat(2, { deck_name: null, deck_card_count: null })])
    expect(startButton(wrapper)?.attributes('disabled')).toBeDefined()
    // Naming them is the point: "someone isn't ready" leaves the host chasing four people.
    expect(blocker(wrapper).text()).toContain('Player 2')
  })

  it('emits start when the host presses it', async () => {
    const wrapper = mountLobby([seat(1), seat(2)])
    await startButton(wrapper)?.trigger('click')
    expect(wrapper.emitted('start')).toHaveLength(1)
  })

  it('gives a non-host no start button at all, only a way out', () => {
    const seats = [seat(1), seat(2, { is_host: false })]
    const wrapper = mountLobby(seats, seats[1])
    expect(startButton(wrapper)).toBeUndefined()
    expect(wrapper.text()).toContain('Leave the table')
    expect(wrapper.text()).not.toContain('Close room')
  })

  it('makes the host confirm before closing the room for everyone', async () => {
    const wrapper = mountLobby([seat(1), seat(2)])
    const close = wrapper.findAll('button').find((b) => b.text().includes('Close room'))
    await close?.trigger('click')

    expect(wrapper.emitted('close')).toBeUndefined()
    const confirm = wrapper.findAll('button').find((b) => b.text() === 'Close it')
    await confirm?.trigger('click')
    expect(wrapper.emitted('close')).toHaveLength(1)
  })
})

describe('PlayLobby seats', () => {
  it("won't let a seat ready up before it has a deck", () => {
    const seats = [seat(1, { deck_name: null, deck_card_count: null })]
    const wrapper = mountLobby(seats, seats[0])
    const ready = wrapper.findAll('button').find((b) => b.text() === "I'm ready")
    expect(ready?.attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain('Load a deck first')
  })

  it('shows a spectator the table without a deck picker', () => {
    const wrapper = mountLobby([seat(1), seat(2)], null)
    expect(wrapper.text()).toContain("You're watching this room")
    expect(wrapper.findComponent({ name: 'PlayDeckPicker' }).exists()).toBe(false)
  })

  it('lets the host remove another seat but never their own', async () => {
    const wrapper = mountLobby([seat(1), seat(2, { is_host: false })])
    const remove = wrapper
      .findAll('button')
      .filter((b) => b.attributes('aria-label')?.startsWith('Remove'))
    expect(remove).toHaveLength(1)
    expect(remove[0]?.attributes('aria-label')).toBe('Remove Player 2')

    await remove[0]?.trigger('click')
    expect(wrapper.emitted('remove')).toEqual([[2]])
  })
})
