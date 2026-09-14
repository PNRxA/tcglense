import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import PlayDeckPicker from '../PlayDeckPicker.vue'

// The picker is the one place a guest and a signed-in player diverge, and the one place three
// very different request bodies come out of one control. So what's pinned is the body each tab
// sends (a wrong `source` is a 422 the user can do nothing about) and that a guest, who has no
// decks to list, doesn't open on an empty tab.

const loadDeck = vi.hoisted(() => vi.fn<(vars: unknown) => Promise<unknown>>())
const auth = vi.hoisted(() => ({ isAuthenticated: true }))
const loadError = vi.hoisted(() => ({ value: null as { message: string } | null }))

vi.mock('@/composables/usePlayRooms', async () => {
  const { ref, computed } = await import('vue')
  return {
    useLoadPlaySeatDeck: () => ({
      mutateAsync: loadDeck,
      isPending: ref(false),
      error: computed(() => loadError.value),
    }),
  }
})
vi.mock('@/composables/useDecks', async () => {
  const { ref } = await import('vue')
  return {
    useDecksQuery: () => ({
      data: ref({
        data: [{ id: 7, name: 'Atraxa', card_count: 100, commanders: [{ name: 'Atraxa' }] }],
      }),
      isPending: ref(false),
    }),
  }
})
vi.mock('@/composables/usePrecons', async () => {
  const { ref } = await import('vue')
  return {
    usePreconsQuery: () => ({
      data: ref({
        data: [
          {
            slug: 'tmc-eldrazi-unbound',
            name: 'Eldrazi Unbound',
            set_code: 'tmc',
            set_name: 'Modern Horizons 3 Commander',
            deck_type: 'Commander Deck',
            card_count: 100,
          },
        ],
      }),
      isPending: ref(false),
    }),
  }
})
vi.mock('@/stores/auth', () => ({ useAuthStore: () => auth }))

function mountPicker(props: Record<string, unknown> = {}) {
  return mount(PlayDeckPicker, {
    props: { game: 'mtg', code: 'ABC234', seatId: 11, seatToken: 'seat-token', ...props },
  })
}

function tab(wrapper: ReturnType<typeof mountPicker>, label: string) {
  const button = wrapper.findAll('[role="tab"]').find((b) => b.text() === label)
  if (!button) throw new Error(`no "${label}" tab`)
  return button
}

beforeEach(() => {
  auth.isAuthenticated = true
  loadError.value = null
  loadDeck.mockReset().mockResolvedValue({})
})

describe('PlayDeckPicker tabs', () => {
  it('opens on your own decks when you have an account', () => {
    const wrapper = mountPicker()
    expect(tab(wrapper, 'My decks').attributes('aria-selected')).toBe('true')
    expect(wrapper.text()).toContain('Atraxa')
  })

  it('opens on precons for a guest, who has no decks of their own', () => {
    auth.isAuthenticated = false
    const wrapper = mountPicker()
    expect(tab(wrapper, 'Precons').attributes('aria-selected')).toBe('true')
    expect(wrapper.text()).toContain('Eldrazi Unbound')
  })

  it('tells a signed-out visitor why the decks tab is empty rather than showing nothing', async () => {
    auth.isAuthenticated = false
    const wrapper = mountPicker()
    await tab(wrapper, 'My decks').trigger('click')
    expect(wrapper.text()).toContain('Sign in to play one of your own decks')
  })
})

describe('PlayDeckPicker loading', () => {
  it('sends a deck id from the decks tab', async () => {
    const wrapper = mountPicker()
    const row = wrapper.findAll('button').find((b) => b.text().includes('Atraxa'))
    await row?.trigger('click')
    await flushPromises()

    expect(loadDeck).toHaveBeenCalledWith({
      game: 'mtg',
      code: 'ABC234',
      seatId: 11,
      seatToken: 'seat-token',
      body: { source: 'deck', deck_id: 7 },
    })
    expect(wrapper.emitted('loaded')).toHaveLength(1)
  })

  it('sends a precon slug from the precons tab', async () => {
    const wrapper = mountPicker()
    await tab(wrapper, 'Precons').trigger('click')
    const row = wrapper.findAll('button').find((b) => b.text().includes('Eldrazi Unbound'))
    await row?.trigger('click')
    await flushPromises()

    expect(loadDeck.mock.calls[0]?.[0]).toMatchObject({
      body: { source: 'precon', slug: 'tmc-eldrazi-unbound' },
    })
  })

  it('sends the pasted text, and not before there is any', async () => {
    const wrapper = mountPicker()
    await tab(wrapper, 'Paste a list').trigger('click')

    const button = wrapper.findAll('button').find((b) => b.text().includes('Load list'))
    expect(button?.attributes('disabled')).toBeDefined()

    await wrapper.get('textarea').setValue('1 Sol Ring')
    await button?.trigger('click')
    await flushPromises()

    expect(loadDeck.mock.calls[0]?.[0]).toMatchObject({
      body: { source: 'text', text: '1 Sol Ring' },
    })
  })

  it("shows the server's complaint — the unresolved names are the whole point of the 422", async () => {
    loadDeck.mockRejectedValue(new Error('nope'))
    loadError.value = { message: "Couldn't resolve: Blak Lotus, Lightening Bolt" }
    const wrapper = mountPicker()
    const row = wrapper.findAll('button').find((b) => b.text().includes('Atraxa'))
    await row?.trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Blak Lotus')
    // A rejected load must not claim success upstream.
    expect(wrapper.emitted('loaded')).toBeUndefined()
  })

  it('says what it would replace when the seat already has a deck', () => {
    const wrapper = mountPicker({ currentDeckName: 'Eldrazi Unbound' })
    expect(wrapper.text()).toContain('pick another to')
  })
})
