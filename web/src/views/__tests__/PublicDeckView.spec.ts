import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import { makeCard } from '@/test/fixtures'
import { useCardSizeStore } from '@/stores/cardSize'
import type { DeckDetail } from '@/lib/api'

// A single shared copy-mutation spy so tests can assert what the copy button dispatched.
const copyMutateAsync = vi.hoisted(() =>
  vi.fn<(vars: unknown) => Promise<unknown>>(async () => ({ game: 'mtg', id: 42 })),
)

// Mutable auth state the mocked store returns; each test sets it before mounting.
const authState = vi.hoisted(() => ({
  sessionResolved: true,
  isAuthenticated: true,
  user: { handle: 'bob-0002' } as { handle: string | null } | null,
}))

// Typed as the real wire shape so DTO drift fails here instead of silently passing.
const deck: DeckDetail = {
  id: 7,
  game: 'mtg',
  name: 'Alice Brew',
  description: null,
  format: null,
  folder_id: null,
  is_public: true,
  handle: 'alice-0001',
  summary: { unique_cards: 1, total_cards: 3, total_value_usd: null, bulk_value_usd: null },
  sections: [{ id: 1, name: 'Creatures', position: 0 }],
  cards: [
    {
      card: makeCard('c1', { name: 'Goblin', color_identity: ['R'] }),
      section_id: 1,
      quantity: 3,
      foil_quantity: 0,
    },
  ],
  created_at: '',
  updated_at: '',
}

vi.mock('@/composables/useDecks', async () => {
  const { ref: vueRef } = await import('vue')
  return {
    usePublicDeckQuery: () => ({
      data: vueRef(deck),
      isPending: vueRef(false),
      isError: vueRef(false),
    }),
    useCopyPublicDeckMutation: () => ({
      mutateAsync: copyMutateAsync,
      isPending: vueRef(false),
    }),
  }
})

vi.mock('@/composables/useCurrency', () => ({
  useCurrency: () => ({ formatUsd: () => '' }),
}))

vi.mock('@/stores/auth', () => ({ useAuthStore: () => authState }))

vi.mock('@/lib/seo', () => ({ usePageMeta: vi.fn<() => void>() }))

import PublicDeckView from '../PublicDeckView.vue'

const PassThrough = defineComponent({ template: '<div><slot /></div>' })
const ButtonStub = defineComponent({
  inheritAttrs: false,
  template: '<button v-bind="$attrs"><slot /></button>',
})

function mountView(pinia = createPinia()) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: PassThrough }],
  })
  const push = vi.spyOn(router, 'push')
  const wrapper = mount(PublicDeckView, {
    props: { handle: 'alice-0001', id: '7' },
    global: {
      // A real Pinia for the card-size store the view reads (issue #562).
      plugins: [router, pinia],
      stubs: {
        Button: ButtonStub,
        LoadingRow: PassThrough,
        CardTile: PassThrough,
        CardSizeMenu: PassThrough,
        DeckSectionNav: PassThrough,
        DeckStats: PassThrough,
      },
    },
  })
  return { wrapper, push }
}

function copyButton(wrapper: ReturnType<typeof mountView>['wrapper']) {
  return wrapper.findAll('button').find((b) => b.text().includes('Copy to my decks'))
}

describe('PublicDeckView copy-to-my-decks', () => {
  it('lets a signed-in visitor copy the deck and routes to the new one', async () => {
    authState.isAuthenticated = true
    authState.user = { handle: 'bob-0002' }
    copyMutateAsync.mockClear()

    const { wrapper, push } = mountView()
    const button = copyButton(wrapper)
    expect(button, 'copy button should be shown').toBeTruthy()

    await button!.trigger('click')
    await flushPromises()

    expect(copyMutateAsync).toHaveBeenCalledExactlyOnceWith({ handle: 'alice-0001', deckId: 7 })
    expect(push).toHaveBeenCalledWith('/decks/mtg/42')
    wrapper.unmount()
  })

  it('hides the copy button from anonymous visitors', () => {
    authState.isAuthenticated = false
    authState.user = null

    const { wrapper } = mountView()
    expect(copyButton(wrapper)).toBeFalsy()
    wrapper.unmount()
  })

  it('hides the copy button on the viewer’s own deck', () => {
    authState.isAuthenticated = true
    authState.user = { handle: 'alice-0001' }

    const { wrapper } = mountView()
    expect(copyButton(wrapper)).toBeFalsy()
    wrapper.unmount()
  })
})

describe('PublicDeckView card display (issue #562)', () => {
  // The card-size preference persists to localStorage; keep tests order-independent.
  afterEach(() => localStorage.removeItem('tcglense_card_size'))

  it('filters the cards client-side with a copy-weighted status line', async () => {
    const { wrapper } = mountView()
    const input = wrapper.get('input[aria-label="Filter cards by name, type, or text"]')

    await input.setValue('zzz-no-match')
    expect(wrapper.text()).toContain('Showing 0 of 3 cards.')
    expect(wrapper.text()).toContain('No cards in this deck match your filter.')

    await wrapper.get('button[aria-label="Filter to red"]').trigger('click')
    await input.setValue('')
    // The Goblin fixture's colour identity is red, so the pip alone matches all copies.
    expect(wrapper.text()).toContain('Showing 3 of 3 cards.')

    const clear = wrapper.findAll('button').find((b) => b.text() === 'Clear filters')
    expect(clear).toBeTruthy()
    await clear!.trigger('click')
    expect(wrapper.text()).not.toContain('Showing')
    wrapper.unmount()
  })

  it('applies the persisted card-size preference to the section grid', () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    useCardSizeStore().setSize('large')

    const { wrapper } = mountView(pinia)
    const grid = wrapper.get('section div.grid')
    expect(grid.classes()).toContain('grid-cols-2')
    wrapper.unmount()
  })
})
