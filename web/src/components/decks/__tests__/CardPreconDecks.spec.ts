import { describe, expect, it, vi } from 'vitest'
import type { Ref } from 'vue'
import { mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { CardPreconRef, Page, PreconDeck } from '@/lib/api'
import CardPreconDecks from '../CardPreconDecks.vue'

// Mocked at the composable seam (the query hook), like the count-control spec: the section's
// own logic — bucket the entries, chip the notable ones, collapse by default — is what's
// under test, not vue-query plumbing.
const h = vi.hoisted(() => ({
  page: null as unknown as Ref<Page<CardPreconRef> | undefined>,
}))

vi.mock('@/composables/usePrecons', async () => {
  const { ref } = await import('vue')
  h.page = ref<Page<CardPreconRef> | undefined>(undefined)
  return { useCardPreconsQuery: () => ({ data: h.page }) }
})

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
    face_card: null,
    ...overrides,
  }
}

function pageOf(entries: CardPreconRef[], total = entries.length): Page<CardPreconRef> {
  return { data: entries, page: 1, page_size: 60, total, has_more: total > entries.length }
}

async function mountSection(id = 'dummy-dmu-0001') {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
  })
  return mount(CardPreconDecks, {
    props: { game: 'mtg', id },
    global: { plugins: [router] },
  })
}

describe('CardPreconDecks', () => {
  it('renders nothing while empty, and a collapsed counted section when decks arrive', async () => {
    const wrapper = await mountSection()
    expect(wrapper.find('section').exists()).toBe(false)

    h.page.value = pageOf([
      {
        precon: precon({}),
        quantity: 1,
        foil: false,
        commander: false,
      },
    ])
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).toContain('Preconstructed decks (1)')
    // Collapsed by default: the heading shows, the tiles wait for a click.
    expect(wrapper.get('button[aria-expanded]').attributes('aria-expanded')).toBe('false')
    expect(wrapper.text()).not.toContain('A Deck')

    await wrapper.get('button[aria-expanded]').trigger('click')
    expect(wrapper.text()).toContain('A Deck')
    expect(wrapper.text()).toContain('Commander Deck')
  })

  it('chips the notable inclusions and reports a truncated listing', async () => {
    const wrapper = await mountSection()
    h.page.value = pageOf(
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

    // The heading counts every containing deck, not just the page shown.
    expect(wrapper.text()).toContain('Preconstructed decks (80)')
    expect(wrapper.text()).toContain('Commander')
    expect(wrapper.text()).toContain('Foil')
    expect(wrapper.text()).toContain('×21')
    expect(wrapper.text()).toContain('Showing the newest 2 of 80')

    // A plain single copy earns no chip at all.
    const plain = pageOf([{ precon: precon({}), quantity: 1, foil: false, commander: false }])
    h.page.value = plain
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).not.toContain('×1')
    expect(wrapper.text()).not.toContain('Showing the newest')
  })
})
