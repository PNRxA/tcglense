import { beforeEach, describe, expect, it, vi } from 'vitest'
import { reactive, type Ref } from 'vue'
import { mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { CardDeckRef, CardPreconRef, Deck, Page, PreconDeck } from '@/lib/api'
import CardDecks from '../CardDecks.vue'

// Mocked at the seams the section reads — the auth store (signed in or not) and the two
// query hooks — so what's under test is its own behaviour: the one "Decks" heading over
// both buckets, hide when signed out or empty, the maybeboard "Considering" label, the
// different-printing note, and the precon chips.
const h = vi.hoisted(() => ({
  auth: null as unknown as { isAuthenticated: boolean },
  owned: null as unknown as Ref<{ data: CardDeckRef[] } | undefined>,
  precons: null as unknown as Ref<Page<CardPreconRef> | undefined>,
}))

vi.mock('@/stores/auth', () => {
  h.auth = reactive({ isAuthenticated: true })
  return { useAuthStore: () => h.auth }
})

vi.mock('@/composables/useDecks', async () => {
  const { ref } = await import('vue')
  h.owned = ref<{ data: CardDeckRef[] } | undefined>(undefined)
  return { useDecksContainingQuery: () => ({ data: h.owned }) }
})

vi.mock('@/composables/usePrecons', async () => {
  const { ref } = await import('vue')
  h.precons = ref<Page<CardPreconRef> | undefined>(undefined)
  return { useCardPreconsQuery: () => ({ data: h.precons }) }
})

// The embedded PreconTile prices its deck through the currency store; mock the composable
// (the PrintingTile spec's idiom) so mounting needs no Pinia.
vi.mock('@/composables/useCurrency', () => ({
  useCurrency: () => ({ formatUsd: (amount: string | null) => (amount ? `$${amount}` : null) }),
}))

const VIEWED_ID = 'dummy-dmb-0080'

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
    value_usd: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-02T00:00:00Z',
    ...overrides,
  }
}

function ownedEntry(overrides: Partial<CardDeckRef>): CardDeckRef {
  return {
    deck: deck({}),
    quantity: 2,
    maybeboard_quantity: 0,
    printings: [{ id: VIEWED_ID, set_code: 'dmb', collector_number: '80', quantity: 2 }],
    ...overrides,
  }
}

function precon(overrides: Partial<PreconDeck>): PreconDeck {
  return {
    slug: 'a-deck-abc',
    game: 'mtg',
    name: 'A Deck',
    set_code: 'abc',
    set_name: 'A Big Cube',
    deck_type: 'Commander Deck',
    released_at: '2024-06-20',
    color_identity: ['W'],
    card_count: 100,
    sideboard_count: 0,
    price_usd: null,
    face_card: null,
    ...overrides,
  }
}

function preconPage(entries: CardPreconRef[], total = entries.length): Page<CardPreconRef> {
  return { data: entries, page: 1, page_size: 60, total, has_more: total > entries.length }
}

async function mountSection() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
  })
  return mount(CardDecks, {
    props: { game: 'mtg', id: VIEWED_ID },
    global: { plugins: [router] },
  })
}

describe('CardDecks', () => {
  beforeEach(() => {
    // The mocked refs outlive a single mount — reset them so no test reads another's data.
    h.auth.isAuthenticated = true
    h.owned.value = undefined
    h.precons.value = undefined
  })

  it('shows one Decks heading over both buckets, own decks open, precons collapsed', async () => {
    const wrapper = await mountSection()
    expect(wrapper.find('section').exists()).toBe(false)

    h.owned.value = { data: [ownedEntry({})] }
    h.precons.value = preconPage([
      { precon: precon({}), quantity: 1, foil: false, commander: false },
    ])
    await wrapper.vm.$nextTick()

    expect(wrapper.get('h2').text()).toBe('Decks')
    expect(wrapper.text()).toContain('In your decks (1)')
    expect(wrapper.text()).toContain('Preconstructed decks (1)')
    const [ownedBucket, preconBucket] = wrapper.findAll('button[aria-expanded]')
    expect(ownedBucket?.attributes('aria-expanded')).toBe('true')
    expect(preconBucket?.attributes('aria-expanded')).toBe('false')
    // The open bucket's rows show; the exact printing on screen earns no printing note.
    expect(wrapper.text()).toContain('Relic pile')
    expect(wrapper.text()).toContain('2 copies')
    expect(wrapper.text()).not.toContain('As DMB')
    const links = wrapper.findAll('a').map((a) => a.attributes('href'))
    expect(links).toContain('/decks/mtg/7')

    await preconBucket?.trigger('click')
    expect(wrapper.text()).toContain('A Deck')
  })

  it('notes a different printing, and labels maybeboard-only decks as considering', async () => {
    const wrapper = await mountSection()
    h.owned.value = {
      data: [
        // Every copy is another printing than the page's.
        ownedEntry({
          printings: [
            { id: 'dummy-dmu-0013', set_code: 'dmu', collector_number: '13', quantity: 2 },
          ],
        }),
        // Both printings: the page's plus another.
        ownedEntry({
          deck: deck({ id: 8, name: 'Second brew' }),
          quantity: 3,
          printings: [
            { id: VIEWED_ID, set_code: 'dmb', collector_number: '80', quantity: 2 },
            { id: 'dummy-dmu-0013', set_code: 'dmu', collector_number: '13', quantity: 1 },
          ],
        }),
        // Maybeboard only.
        ownedEntry({
          deck: deck({ id: 9, name: 'Someday pile' }),
          quantity: 0,
          maybeboard_quantity: 4,
        }),
      ],
    }
    await wrapper.vm.$nextTick()

    expect(wrapper.text()).toContain('As DMU #13')
    expect(wrapper.text()).toContain('Also as DMU #13')
    expect(wrapper.text()).toContain('Considering')
    expect(wrapper.text()).not.toContain('0 copies')
  })

  it('chips the notable precon inclusions and reports a truncated listing', async () => {
    const wrapper = await mountSection()
    h.precons.value = preconPage(
      [
        {
          precon: precon({ slug: 'lead-abc', name: 'Lead' }),
          quantity: 1,
          foil: true,
          commander: true,
        },
        {
          precon: precon({ slug: 'pile-abc', name: 'Pile' }),
          quantity: 21,
          foil: false,
          commander: false,
        },
      ],
      80,
    )
    await wrapper.vm.$nextTick()
    await wrapper.get('button[aria-expanded]').trigger('click')

    // The bucket heading counts every containing deck, not just the page shown.
    expect(wrapper.text()).toContain('Preconstructed decks (80)')
    expect(wrapper.text()).toContain('Commander')
    expect(wrapper.text()).toContain('Foil')
    expect(wrapper.text()).toContain('×21')
    expect(wrapper.text()).toContain('Showing the newest 2 of 80')
  })

  it('renders nothing when signed out and no precon has the card', async () => {
    const wrapper = await mountSection()
    h.owned.value = { data: [ownedEntry({})] }
    h.precons.value = preconPage([])
    h.auth.isAuthenticated = false
    await wrapper.vm.$nextTick()
    expect(wrapper.find('section').exists()).toBe(false)
    h.auth.isAuthenticated = true
  })
})
