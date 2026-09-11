import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import { makeCard } from '@/test/fixtures'
import { useDeckViewStore } from '@/stores/deckView'
import type { OwnedCountsMap, PreconDeckDetail } from '@/lib/api'

// Mutable auth state the mocked store returns; each test sets it before mounting.
const authState = vi.hoisted(() => ({
  sessionResolved: true,
  isAuthenticated: true,
  user: { handle: 'bob-0002' } as { handle: string | null } | null,
}))

// What the two batched holdings reads answer. The real seam empties both while signed out;
// the mock mirrors that, since the view relies on it rather than gating the chips itself.
const holdings = vi.hoisted(() => ({
  owned: {} as OwnedCountsMap,
  wanted: {} as OwnedCountsMap,
}))

// Typed as the real wire shape so DTO drift fails here instead of silently passing.
const precon: PreconDeckDetail = {
  slug: 'eldrazi-unbound',
  game: 'mtg',
  name: 'Eldrazi Unbound',
  set_code: 'cmm',
  set_name: 'Commander Masters',
  deck_type: 'Commander Deck',
  format: 'commander',
  released_at: '2023-08-04',
  color_identity: [],
  card_count: 2,
  sideboard_count: 0,
  price_usd: null,
  face_card: null,
  summary: { unique_cards: 2, total_cards: 2, total_value_usd: null, bulk_value_usd: null },
  sideboard_summary: {
    unique_cards: 0,
    total_cards: 0,
    total_value_usd: null,
    bulk_value_usd: null,
  },
  product: null,
  cards: [
    { card: makeCard('c1', { name: 'Zhulodok' }), board: 'commander', quantity: 1, foil: true },
    { card: makeCard('c2', { name: 'Sol Ring' }), board: 'main', quantity: 1, foil: false },
  ],
}

vi.mock('@/composables/usePrecons', async () => {
  const { ref: vueRef } = await import('vue')
  return {
    usePreconQuery: () => ({
      data: vueRef(precon),
      isPending: vueRef(false),
      isError: vueRef(false),
    }),
    useCopyPreconMutation: () => ({
      mutateAsync: vi.fn<() => Promise<unknown>>(),
      isPending: vueRef(false),
    }),
    useAddPreconToCollectionMutation: () => ({
      mutateAsync: vi.fn<() => Promise<unknown>>(),
      isPending: vueRef(false),
    }),
  }
})

vi.mock('@/composables/useCollection', async () => {
  const { computed: vueComputed } = await import('vue')
  return {
    useOwnedCounts: () => ({
      ownership: vueComputed(() => (authState.isAuthenticated ? holdings.owned : {})),
    }),
  }
})
vi.mock('@/composables/useWishlist', async () => {
  const { computed: vueComputed } = await import('vue')
  return {
    useWishlistCounts: () => ({
      ownership: vueComputed(() => (authState.isAuthenticated ? holdings.wanted : {})),
    }),
  }
})

// The analysis panels are the server's and have their own specs; stub the precon reads so
// this view's own wiring is what's under test and no query client is needed.
vi.mock('@/composables/useDeckAnalysis', async () => {
  const { ref: vueRef } = await import('vue')
  const settled = () => ({
    isPending: vueRef(false),
    isFetching: vueRef(false),
    isError: vueRef(false),
    isLoadingError: vueRef(false),
    isRefetchError: vueRef(false),
    error: vueRef(null),
  })
  return {
    usePreconLegalityQuery: () => ({ data: vueRef({ data: null }), ...settled() }),
    usePreconBracketQuery: () => ({ data: vueRef({ data: null }), ...settled() }),
    usePreconStatsQuery: () => ({ data: vueRef(undefined), ...settled() }),
    usePreconGoldfishQuery: () => ({ data: vueRef(undefined), ...settled() }),
    usePreconTokensQuery: () => ({ data: vueRef(undefined), ...settled() }),
    usePreconCombosQuery: () => ({ data: vueRef(undefined), ...settled() }),
    usePreconManaQuery: () => ({ data: vueRef(undefined), ...settled() }),
    usePreconRolesQuery: () => ({ data: vueRef(undefined), ...settled() }),
  }
})

vi.mock('@/composables/useCatalog', async () => {
  const { ref: vueRef } = await import('vue')
  return { useGameName: () => vueRef('Magic: The Gathering') }
})
vi.mock('@/composables/useCurrency', () => ({
  useCurrency: () => ({ formatUsd: () => '' }),
}))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => authState }))
vi.mock('@/lib/seo', () => ({ usePageMeta: vi.fn<() => void>() }))

import PreconDeckView from '../PreconDeckView.vue'

const PassThrough = defineComponent({ template: '<div><slot /></div>' })
// The tile stub keeps the `#badge` slot the ownership chips render through.
const CardTileStub = defineComponent({
  template: '<div class="tile"><slot /><slot name="badge" /></div>',
})

function mountView(mode: 'grid' | 'list' = 'grid') {
  const pinia = createPinia()
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: PassThrough }],
  })
  const wrapper = mount(PreconDeckView, {
    props: { game: 'mtg', slug: 'eldrazi-unbound' },
    global: {
      plugins: [router, pinia],
      stubs: {
        LoadingRow: PassThrough,
        CardTile: CardTileStub,
        CardSizeMenu: PassThrough,
        PageBreadcrumbs: PassThrough,
        DeckSectionNav: PassThrough,
        DeckOverview: PassThrough,
        DeckCombos: PassThrough,
        DeckTokens: PassThrough,
      },
    },
  })
  useDeckViewStore(pinia).setMode(mode)
  return wrapper
}

function ownershipChips(wrapper: ReturnType<typeof mountView>) {
  return wrapper
    .findAll('[role="img"][aria-label^="You"]')
    .map((chip) => chip.attributes('aria-label'))
}

afterEach(() => {
  authState.isAuthenticated = true
  holdings.owned = {}
  holdings.wanted = {}
})

describe('PreconDeckView ownership chips (issue #707)', () => {
  it('shows how many of each card the signed-in reader owns and wants, on the grid', async () => {
    holdings.owned = {
      c1: { quantity: 1, foil_quantity: 1 },
      c2: { quantity: 4, foil_quantity: 0 },
    }
    holdings.wanted = { c2: { quantity: 0, foil_quantity: 1 } }

    const wrapper = mountView('grid')
    await wrapper.vm.$nextTick()

    expect(ownershipChips(wrapper)).toEqual([
      'You own 2 of this card',
      'You own 4 of this card',
      'You have 1 of this card on your wish list',
    ])
    wrapper.unmount()
  })

  it('carries the same chips on the compact list rows', async () => {
    holdings.owned = { c2: { quantity: 3, foil_quantity: 0 } }

    const wrapper = mountView('list')
    await wrapper.vm.$nextTick()

    expect(ownershipChips(wrapper)).toEqual(['You own 3 of this card'])
    // Beside the deck's own count, never in place of it.
    expect(wrapper.text()).toContain('×1')
    wrapper.unmount()
  })

  it('renders no chip for a card the reader neither owns nor wants', async () => {
    holdings.owned = { c1: { quantity: 1, foil_quantity: 0 } }

    const wrapper = mountView('grid')
    await wrapper.vm.$nextTick()

    expect(ownershipChips(wrapper)).toEqual(['You own 1 of this card'])
    wrapper.unmount()
  })

  // A precon page is public: signed out there is no collection to count against, and the
  // page must read exactly as it did before the chips existed.
  it('shows nothing to an anonymous visitor', async () => {
    authState.isAuthenticated = false
    holdings.owned = { c1: { quantity: 1, foil_quantity: 0 } }

    const wrapper = mountView('grid')
    await wrapper.vm.$nextTick()

    expect(ownershipChips(wrapper)).toEqual([])
    wrapper.unmount()
  })
})
