import { describe, expect, it, vi } from 'vitest'
import { reactive, type Ref } from 'vue'
import { mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { CardDeckRef, Deck } from '@/lib/api'
import CardOwnedDecks from '../CardOwnedDecks.vue'

// Mocked at the two seams the section reads — the auth store (signed in or not) and the
// containment query — so what's under test is its own behaviour: hide when signed out or
// empty, open by default, and word "runs it" apart from "only considering it".
const h = vi.hoisted(() => ({
  auth: null as unknown as { isAuthenticated: boolean },
  data: null as unknown as Ref<{ data: CardDeckRef[] } | undefined>,
}))

vi.mock('@/stores/auth', () => {
  h.auth = reactive({ isAuthenticated: true })
  return { useAuthStore: () => h.auth }
})

vi.mock('@/composables/useDecks', async () => {
  const { ref } = await import('vue')
  h.data = ref<{ data: CardDeckRef[] } | undefined>(undefined)
  return { useDecksContainingQuery: () => ({ data: h.data }) }
})

function deck(overrides: Partial<Deck>): Deck {
  return {
    id: 7,
    game: 'mtg',
    name: 'Relic pile',
    description: null,
    format: 'commander',
    folder_id: null,
    is_public: false,
    card_count: 99,
    color_identity: ['W', 'U'],
    commanders: [],
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-02T00:00:00Z',
    ...overrides,
  }
}

async function mountSection() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
  })
  return mount(CardOwnedDecks, {
    props: { game: 'mtg', id: 'dummy-dmb-0080' },
    global: { plugins: [router] },
  })
}

describe('CardOwnedDecks', () => {
  it('opens by default with the count, linking each deck', async () => {
    const wrapper = await mountSection()
    expect(wrapper.find('section').exists()).toBe(false)

    h.data.value = {
      data: [
        { deck: deck({}), quantity: 2, maybeboard_quantity: 0 },
        {
          deck: deck({ id: 8, name: 'Second brew', format: null }),
          quantity: 1,
          maybeboard_quantity: 3,
        },
      ],
    }
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).toContain('In your decks (2)')
    expect(wrapper.get('button[aria-expanded]').attributes('aria-expanded')).toBe('true')
    expect(wrapper.text()).toContain('Relic pile')
    expect(wrapper.text()).toContain('2 copies')
    // Running it AND considering more: both counts, one chip.
    expect(wrapper.text()).toContain('1 copy · 3 considered')
    const links = wrapper.findAll('a').map((a) => a.attributes('href'))
    expect(links).toContain('/decks/mtg/7')
    expect(links).toContain('/decks/mtg/8')
  })

  it('labels a maybeboard-only deck as considering, not as running the card', async () => {
    const wrapper = await mountSection()
    h.data.value = {
      data: [{ deck: deck({}), quantity: 0, maybeboard_quantity: 4 }],
    }
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).toContain('Considering')
    expect(wrapper.text()).not.toContain('0 copies')
  })

  it('renders nothing when signed out, even with cached data', async () => {
    const wrapper = await mountSection()
    h.data.value = { data: [{ deck: deck({}), quantity: 2, maybeboard_quantity: 0 }] }
    h.auth.isAuthenticated = false
    await wrapper.vm.$nextTick()
    expect(wrapper.find('section').exists()).toBe(false)
    h.auth.isAuthenticated = true
  })
})
