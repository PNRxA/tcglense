import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import type { ProductCardSection } from '@/lib/api'
import ProductCards from '../ProductCards.vue'
import ProductCardsSection from '../ProductCardsSection.vue'

// Drive the parent off a controlled sections manifest + search state, stubbing the manifest
// query and the URL-backed search composable so no QueryClient / router / Pinia is needed —
// the unit under test is which section blocks render (issue #224) and how the search box gates
// them (issue #222). The per-section pagination lives in the stubbed ProductCardsSection child.
const state = vi.hoisted(() => ({
  manifest: [] as ProductCardSection[],
  query: '',
  sort: 'default',
  error: undefined as unknown,
}))

vi.mock('@/composables/useProducts', () => ({
  PRODUCT_CARDS_PAGE_SIZE: 60,
  useProductCardSectionsQuery: () => ({
    data: {
      get value() {
        return { data: state.manifest }
      },
    },
    error: {
      get value() {
        return state.error
      },
    },
  }),
  useProductCardsQuery: () => ({ data: { value: { data: [], total: 0 } } }),
}))
vi.mock('@/composables/useProductCardsSearch', async () => {
  const { computed, ref } = await import('vue')
  return {
    useProductCardsSearch: () => ({
      searchInput: ref(state.query),
      query: computed(() => state.query),
      sort: ref(state.sort),
    }),
  }
})
vi.mock('@/composables/useCardSearch', () => ({
  // A truthy error means "malformed search"; the exact message is the real fn's concern.
  searchErrorMessage: (error: unknown) => (error ? 'Malformed search.' : null),
}))

function section(key: string, total = 1): ProductCardSection {
  return { key, total }
}

// Mount over a manifest (+ optional search state) and return the section blocks the parent
// rendered — each block's section key, heading, and the search threaded into it, in order.
function mountCards(
  manifest: ProductCardSection[],
  productType: string,
  opts: { query?: string; sort?: string; error?: unknown } = {},
) {
  state.manifest = manifest
  state.query = opts.query ?? ''
  state.sort = opts.sort ?? 'default'
  state.error = opts.error
  const wrapper = mount(ProductCards, {
    props: { game: 'mtg', id: '100', productType },
    global: {
      stubs: {
        ProductCardsSection: true,
        CardSearchBox: true,
        SearchSyntaxHint: true,
        AdvancedSearchPanel: true,
        CardSizeMenu: true,
        CardSortMenu: true,
      },
    },
  })
  return {
    wrapper,
    sections: wrapper.findAllComponents(ProductCardsSection).map((c) => ({
      key: c.props('sectionKey') as string,
      title: c.props('title') as string,
      search: c.props('search') as string,
      sort: c.props('sort') as string,
    })),
  }
}

beforeEach(() => {
  state.manifest = []
  state.query = ''
  state.sort = 'default'
  state.error = undefined
})

describe('ProductCards sections', () => {
  it('renders one block per manifest section, in order, with the right headings', () => {
    const { sections } = mountCards(
      [section('contains'), section('exclusive'), section('booster'), section('variable')],
      'collector_pack',
    )
    expect(sections.map((s) => s.key)).toEqual(['contains', 'exclusive', 'booster', 'variable'])
    expect(sections.map((s) => s.title)).toEqual([
      'In the box',
      'Collector Booster exclusives',
      'Can be pulled from boosters',
      'May be included',
    ])
  })

  it('labels the exclusives block by the product’s own booster family', () => {
    const { sections } = mountCards([section('exclusive'), section('booster')], 'play_pack')
    expect(sections.map((s) => s.title)).toEqual([
      'Play Booster exclusives',
      'Can be pulled from boosters',
    ])
  })

  it('falls back to a generic exclusives label with no booster family', () => {
    const { sections } = mountCards([section('exclusive')], 'bundle')
    expect(sections.map((s) => s.title)).toEqual(['Booster exclusives'])
  })

  it('sums the section counts into the heading total', () => {
    const { wrapper, sections } = mountCards(
      [section('contains', 2), section('booster', 3)],
      'bundle',
    )
    expect(sections.map((s) => s.key)).toEqual(['contains', 'booster'])
    // "Cards in this product (5)" — the grand total across sections (from the manifest).
    expect(wrapper.find('h2').text()).toContain('5')
  })

  it('renders nothing when the product has no card sections', () => {
    const { wrapper, sections } = mountCards([], 'bundle')
    expect(sections).toHaveLength(0)
    expect(wrapper.find('section').exists()).toBe(false)
  })

  it('threads the committed search into each block', () => {
    const { sections } = mountCards([section('booster')], 'bundle', { query: 't:goblin' })
    expect(sections.map((s) => s.search)).toEqual(['t:goblin'])
  })

  it('threads the committed sort into every block (so the sections re-order together)', () => {
    const { sections } = mountCards([section('contains'), section('booster')], 'bundle', {
      sort: 'price:desc',
    })
    expect(sections.map((s) => s.sort)).toEqual(['price:desc', 'price:desc'])
  })

  it('keeps the search box up and shows a no-match note when a search matches nothing', () => {
    const { wrapper, sections } = mountCards([], 'bundle', { query: 'zzznope' })
    // The section (and thus the search box) stays mounted so the filter can be cleared…
    expect(wrapper.find('section').exists()).toBe(true)
    // …but no blocks render, replaced by the note.
    expect(sections).toHaveLength(0)
    expect(wrapper.text()).toContain('No cards match')
  })

  it('surfaces a malformed-search error instead of the blocks', () => {
    const { wrapper, sections } = mountCards([], 'bundle', { query: 'bad:', error: new Error('x') })
    expect(sections).toHaveLength(0)
    expect(wrapper.text()).toContain('Malformed search.')
    expect(wrapper.text()).not.toContain('No cards match')
  })
})
