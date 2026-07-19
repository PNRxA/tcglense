import { describe, it, expect, beforeEach, vi } from 'vitest'
import { ref } from 'vue'
import { mount, RouterLinkStub } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { CardSet, ProductHoldingSet, ProductHoldingSummary } from '@/lib/api'
import { makeCardSet } from '@/test/fixtures'
import ProductHoldingSection from '@/components/products/ProductHoldingSection.vue'

// Drive the section off controlled query state rather than the network: the held-set list, the
// product summary, the catalog set list, and the currency formatter are all mocked, so the test
// exercises render gating, the summary-sourced header count, one tile per set (real tiles) with
// their link targets, and the View-all link — not the query layer (covered by the API path +
// backend tests). The tiles render for real, so their names/counts/values are asserted here too.
const state = vi.hoisted(() => ({
  sets: undefined as { data: ProductHoldingSet[] } | undefined,
  summary: undefined as ProductHoldingSummary | undefined,
  catalog: { data: [] as CardSet[] },
}))

vi.mock('@/composables/useCollection', () => ({
  useCollectionProductSetsQuery: () => ({ data: ref(state.sets) }),
  useCollectionProductSummaryQuery: () => ({ data: ref(state.summary) }),
}))

vi.mock('@/composables/useWishlist', () => ({
  useWishlistProductSetsQuery: () => ({ data: ref(state.sets) }),
  useWishlistProductSummaryQuery: () => ({ data: ref(state.summary) }),
}))

vi.mock('@/composables/useCatalog', () => ({
  useSetsQuery: () => ({ data: ref(state.catalog) }),
}))

// A deterministic money formatter: USD passthrough, null-in → null-out (so the value stat hides).
vi.mock('@/composables/useCurrency', () => ({
  useCurrency: () => ({
    formatUsd: (raw: string | null | undefined) => (raw == null ? null : `$${raw}`),
  }),
}))

function makeProductSet(
  code: string,
  name: string | null,
  over: Partial<ProductHoldingSet> = {},
): ProductHoldingSet {
  return { code, name, unique_products: 1, total_products: 1, total_value_usd: null, ...over }
}

function summary(uniqueProducts: number): ProductHoldingSummary {
  return { unique_products: uniqueProducts, total_products: uniqueProducts, total_value_usd: null }
}

function mountSection(list: 'collection' | 'wishlist' = 'wishlist') {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
  })
  return mount(ProductHoldingSection, {
    props: { game: 'mtg', list },
    global: { plugins: [router], stubs: { RouterLink: RouterLinkStub } },
  })
}

type Wrapper = ReturnType<typeof mountSection>

/** The set tiles (excludes the header's "Browse sealed" / "View all" links, which carry no set). */
function tileLinks(wrapper: Wrapper) {
  return wrapper
    .findAllComponents(RouterLinkStub)
    .filter((link) => String(link.props('to')).includes('/products/sets/'))
}

function findLinkByText(wrapper: Wrapper, text: string) {
  return wrapper.findAllComponents(RouterLinkStub).find((link) => link.text().includes(text))
}

beforeEach(() => {
  state.sets = undefined
  state.summary = undefined
  state.catalog = { data: [] }
})

describe('ProductHoldingSection', () => {
  it('renders nothing while the sets query is undefined', () => {
    state.sets = undefined
    expect(mountSection().find('section').exists()).toBe(false)
  })

  it('renders nothing when the surface holds no sealed products', () => {
    // Zero sets ⇒ the section is hidden entirely.
    state.sets = { data: [] }
    expect(mountSection().find('section').exists()).toBe(false)
  })

  it('renders one tile per set in server order, linking each to the set-scoped list', () => {
    state.sets = {
      data: [
        makeProductSet('aaa', 'Alpha'),
        makeProductSet('bbb', 'Beta'),
        makeProductSet('ccc', 'Gamma'),
      ],
    }
    state.summary = summary(3)
    const wrapper = mountSection('collection')

    const links = tileLinks(wrapper)
    expect(links.map((link) => link.find('p.font-medium').text())).toEqual([
      'Alpha',
      'Beta',
      'Gamma',
    ])
    expect(links.map((link) => link.props('to'))).toEqual([
      '/collection/mtg/products/sets/aaa',
      '/collection/mtg/products/sets/bbb',
      '/collection/mtg/products/sets/ccc',
    ])
  })

  it('links tiles under the wishlist surface for a wish list', () => {
    state.sets = { data: [makeProductSet('aaa', 'Alpha')] }
    const wrapper = mountSection('wishlist')
    expect(tileLinks(wrapper)[0]!.props('to')).toBe('/wishlist/mtg/products/sets/aaa')
  })

  it("shows each tile's name, product count, and value", () => {
    state.sets = {
      data: [
        makeProductSet('abc', 'ABC Set', {
          unique_products: 3,
          total_products: 3,
          total_value_usd: '12.50',
        }),
      ],
    }
    state.summary = summary(3)
    const tile = tileLinks(mountSection())[0]!
    expect(tile.text()).toContain('ABC Set')
    expect(tile.text()).toContain('3 products')
    expect(tile.text()).toContain('$12.50')
  })

  it('falls back to the upper-cased code when a set has no catalog name', () => {
    state.sets = { data: [makeProductSet('xyz', null)] }
    state.summary = summary(1)
    const tile = tileLinks(mountSection())[0]!
    expect(tile.find('p.font-medium').text()).toBe('XYZ')
  })

  it('resolves the catalog set for the tile icon + release date', () => {
    state.sets = { data: [makeProductSet('blb', 'Bloomburrow')] }
    state.summary = summary(1)
    state.catalog = {
      data: [
        makeCardSet('blb', {
          released_at: '2024-08-02',
          icon_svg_uri: 'https://example.test/blb.svg',
        }),
      ],
    }
    const tile = tileLinks(mountSection())[0]!
    // The release date (year) and the icon both ride along from the resolved catalog set.
    expect(tile.text()).toContain('2024')
    expect(tile.find('img').exists()).toBe(true)
  })

  it('shows the summary-sourced unique / total / value stats under the heading', () => {
    state.sets = { data: [makeProductSet('abc', 'ABC Set')] }
    state.summary = { unique_products: 2, total_products: 5, total_value_usd: '12.50' }
    const dl = mountSection('collection').get('dl')
    expect(dl.text()).toContain('Unique products')
    expect(dl.text()).toContain('2')
    expect(dl.text()).toContain('Total products')
    expect(dl.text()).toContain('5')
    expect(dl.text()).toContain('Products value')
    expect(dl.text()).toContain('$12.50')
  })

  it('hides the stats when the summary value is unpriced', () => {
    state.sets = { data: [makeProductSet('abc', 'ABC Set')] }
    // total_value_usd null ⇒ the "Products value" stat self-hides; the two counts remain.
    state.summary = summary(2)
    const dl = mountSection('wishlist').get('dl')
    expect(dl.text()).toContain('Unique products')
    expect(dl.text()).not.toContain('Products value')
  })

  it('hides the stats list until the summary has data', () => {
    state.sets = { data: [makeProductSet('abc', 'ABC Set')] }
    state.summary = undefined
    const wrapper = mountSection()
    // The section renders (a set is held) but the stats list waits for the summary.
    expect(wrapper.find('section').exists()).toBe(true)
    expect(wrapper.find('dl').exists()).toBe(false)
  })

  it('renders a View all link to the surface products list', () => {
    state.sets = { data: [makeProductSet('abc', 'ABC Set')] }
    expect(findLinkByText(mountSection('collection'), 'View all')?.props('to')).toBe(
      '/collection/mtg/products',
    )
    expect(findLinkByText(mountSection('wishlist'), 'View all')?.props('to')).toBe(
      '/wishlist/mtg/products',
    )
  })
})
