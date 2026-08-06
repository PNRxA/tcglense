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

function section(key: string, total = 1, boosterFamily: string | null = null): ProductCardSection {
  return { key, total, booster_family: boosterFamily }
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
      count: c.props('count') as number,
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
      'Guaranteed cards',
      'Exclusive to Collector Booster',
      // "Shared", not "Collector Booster pull pool": the exclusives split out above are part of
      // that pool too, so this block is its remainder — the page must not state two sizes for
      // one named pool.
      'Shared Collector Booster pool',
      'May be included',
    ])
  })

  it('titles the exclusives block by the backend-provided contained booster family', () => {
    // A bundle's own product_type carries no family, but the backend names the collector
    // booster it wraps — so the section reads "Exclusive to Collector Booster" (issue #290).
    // The shared pool below it spans every family, so it stays generic on a bundle.
    const { sections } = mountCards(
      [section('exclusive', 1, 'collector_pack'), section('booster')],
      'bundle',
    )
    expect(sections.map((s) => s.title)).toEqual([
      'Exclusive to Collector Booster',
      'Shared booster pool',
    ])
  })

  it('falls back to the product’s own booster family when the backend hands none down', () => {
    const { sections } = mountCards([section('exclusive'), section('booster')], 'play_pack')
    expect(sections.map((s) => s.title)).toEqual([
      'Exclusive to Play Booster',
      'Shared Play Booster pool',
    ])
  })

  it('falls back to a generic exclusives label with no booster family at all', () => {
    const { sections } = mountCards([section('exclusive')], 'bundle')
    // Never "this booster" — the viewed product is a bundle, which is not one.
    expect(sections.map((s) => s.title)).toEqual(["Exclusive to this product's boosters"])
  })

  it('names an unrecognised section key as a possible-cards bucket, never a containment claim', () => {
    // Mirrors the server, which files an unknown membership into the weakest bucket — the raw
    // wire slug must never surface as a heading.
    const { sections } = mountCards([section('someday')], 'bundle')
    expect(sections.map((s) => s.title)).toEqual(['Possible cards'])
  })

  it('sums the section counts into the heading total', () => {
    const { wrapper, sections } = mountCards(
      [section('contains', 2), section('booster', 3)],
      'bundle',
    )
    expect(sections.map((s) => s.key)).toEqual(['contains', 'booster'])
    // The grand total across sections (from the manifest).
    expect(wrapper.find('h2').text()).toContain('5')
  })

  it('threads each manifest count into its block (labels the collapsed header, #291)', () => {
    const { sections } = mountCards([section('contains', 2), section('booster', 3)], 'bundle')
    expect(sections.map((s) => s.count)).toEqual([2, 3])
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

// The heading is the string that used to lie: it read "Cards in this product (600)" over a
// booster's pull pool. It now takes its noun from the certainties the manifest holds.
describe('ProductCards heading', () => {
  const headingOf = (manifest: ProductCardSection[], productType = 'bundle') =>
    mountCards(manifest, productType).wrapper.find('h2').text().replace(/\s+/g, ' ').trim()

  it('keeps the containment wording when every card really is guaranteed', () => {
    expect(headingOf([section('contains', 99)], 'commander_deck')).toBe(
      'Cards in this product (99)',
    )
  })

  it('calls a booster’s pool a pool, and puts the unit inside the number', () => {
    // The reported bug: a pack holding ~15 cards announced its ~600-card pull pool as its
    // contents. Both the noun and the parenthetical have to change.
    const heading = headingOf([section('booster', 600)], 'play_pack')
    expect(heading).toBe('What you can pull (600-card pool)')
    expect(heading).not.toContain('in this product')
  })

  it('counts the family-exclusive slice into the same pool, never as extra cards', () => {
    expect(headingOf([section('exclusive', 80), section('booster', 520)], 'collector_pack')).toBe(
      'What you can pull (600-card pool)',
    )
  })

  it('hedges anything randomized with one voice', () => {
    expect(headingOf([section('variable', 12)])).toBe('What you might get (12)')
    expect(headingOf([section('booster', 10), section('variable', 2)])).toBe(
      'What you might get (12)',
    )
  })

  it('drops the pool-size claim while a search narrows the manifest', () => {
    // Filtered, the number is what matched — asserting "(3-card pool)" over a 600-card pool
    // would be a brand-new lie.
    const filtered = mountCards([section('booster', 3)], 'play_pack', { query: 't:goblin' })
    expect(filtered.wrapper.find('h2').text().replace(/\s+/g, ' ').trim()).toBe(
      'What you can pull (3)',
    )
  })

  it('names both certainties on a mixed product, and spells the split out below', () => {
    const { wrapper } = mountCards([section('contains', 2), section('booster', 600)], 'bundle')
    expect(wrapper.find('h2').text().replace(/\s+/g, ' ').trim()).toBe(
      "What's guaranteed, what's random (602)",
    )
    expect(wrapper.find('h2 + p').text()).toBe(
      '2 guaranteed · 600 in the pull pool — a copy opens some of the pool, not all of it.',
    )
  })

  it('gives a single-certainty product no reconciling line', () => {
    expect(
      mountCards([section('booster', 5)], 'play_pack')
        .wrapper.find('h2 + p')
        .exists(),
    ).toBe(false)
  })
})
