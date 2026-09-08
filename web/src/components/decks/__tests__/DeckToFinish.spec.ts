import { describe, expect, it, vi } from 'vitest'
import { defineComponent, ref, type Ref } from 'vue'
import { mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { NeededCards } from '@/lib/api'

// What the mocked needed query answers, and the deck id it was asked for.
const state = vi.hoisted(() => ({
  response: null as NeededCards | null,
  askedDeckId: null as number | null,
}))

vi.mock('@/composables/useDecks', () => ({
  useNeededCardsQuery: (_game: Ref<string>, _mode: Ref<string>, deckId: Ref<number | null>) => {
    state.askedDeckId = deckId.value
    return { data: ref(state.response) }
  },
}))

vi.mock('@/composables/useCurrency', () => ({
  useCurrency: () => ({
    formatUsd: (raw: string | null | undefined) => (raw == null ? null : `$${raw}`),
  }),
}))

import DeckToFinish from '../DeckToFinish.vue'

const PassThrough = defineComponent({ template: '<div><slot /></div>' })

function mountLine() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: PassThrough }],
  })
  return mount(DeckToFinish, {
    props: { game: 'mtg', deckId: 7 },
    global: { plugins: [router] },
  })
}

function response(over: Partial<NeededCards['totals']>): NeededCards {
  return {
    data: [],
    deck: { id: 7, name: 'Mine' },
    totals: {
      cards: 0,
      copies: 0,
      held_usd: null,
      held_unpriced_cards: 0,
      cheapest_usd: null,
      cheapest_unpriced_cards: 0,
      ...over,
    },
  }
}

describe('DeckToFinish', () => {
  it('asks for this deck only and links to the list scoped to it', () => {
    state.response = response({ cards: 12, copies: 15, held_usd: '41.20', cheapest_usd: '28.10' })
    const wrapper = mountLine()
    expect(state.askedDeckId).toBe(7)
    expect(wrapper.text()).toContain(
      'To finish: 12 cards · 15 copies · ~$41.20 at your printings · from $28.10 at cheapest printings',
    )
    const link = wrapper.find('a')
    expect(link.attributes('href')).toBe('/decks/mtg/needed?deck=7')
    wrapper.unmount()
  })

  it('says so when nothing is missing, and nothing at all before the answer lands', () => {
    state.response = response({})
    const owned = mountLine()
    expect(owned.text()).toContain('Every card is in your collection')
    expect(owned.find('a').exists()).toBe(false)
    owned.unmount()

    state.response = null
    const pending = mountLine()
    expect(pending.text()).toBe('')
    pending.unmount()
  })
})
