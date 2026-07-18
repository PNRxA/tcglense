import { describe, it, expect, beforeEach, vi } from 'vitest'
import { nextTick, ref, type Ref } from 'vue'
import { flushPromises, mount, RouterLinkStub } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type {
  ProductHoldingEntry,
  ProductHoldingSetGroup,
  ProductHoldingSetPage,
  ProductHoldingSummary,
} from '@/lib/api'
import ProductHoldingSection from '@/components/products/ProductHoldingSection.vue'
import CardPagination from '@/components/cards/CardPagination.vue'

// Drive the section off controlled query state rather than the network: the by-set list, the
// product summary, the other-surface counts, and the currency formatter are all mocked, so the
// test exercises render gating, the summary-sourced header count, the per-group blocks, both
// holding maps handed to every grid, and the page-when-needed rule — not the query layer
// (covered by the API path + backend tests).
// `page` and `dataRef` capture the real refs handed to the by-set mock on the most recent mount,
// so a test can mutate them after mounting to exercise clamp-page reactivity — the other fields
// are only read as seed values at mount time.
const state = vi.hoisted(() => ({
  data: undefined as ProductHoldingSetPage | undefined,
  summary: undefined as ProductHoldingSummary | undefined,
  isPlaceholderData: false,
  isSuccess: true,
  page: undefined as Ref<number> | undefined,
  dataRef: undefined as Ref<ProductHoldingSetPage | undefined> | undefined,
  otherCounts: {} as Record<string, { quantity: number; foil_quantity: number }>,
}))

// Bodies are inlined per factory because `vi.mock` is hoisted above any module-level const, so
// it can only reach the (hoisted) `state` and the imported `ref` — not helpers declared below.
vi.mock('@/composables/useWishlist', () => ({
  useWishlistProductsBySetQuery: (_game: unknown, page: Ref<number>) => {
    state.page = page
    const dataRef = ref(state.data) as Ref<ProductHoldingSetPage | undefined>
    state.dataRef = dataRef
    return {
      data: dataRef,
      isPlaceholderData: ref(state.isPlaceholderData),
      isSuccess: ref(state.isSuccess),
    }
  },
  useWishlistProductSummaryQuery: () => ({ data: ref(state.summary) }),
  useWishlistProductCounts: () => ({ ownership: ref(state.otherCounts) }),
}))

vi.mock('@/composables/useCollection', () => ({
  useCollectionProductsBySetQuery: (_game: unknown, page: Ref<number>) => {
    state.page = page
    const dataRef = ref(state.data) as Ref<ProductHoldingSetPage | undefined>
    state.dataRef = dataRef
    return {
      data: dataRef,
      isPlaceholderData: ref(state.isPlaceholderData),
      isSuccess: ref(state.isSuccess),
    }
  },
  useCollectionProductSummaryQuery: () => ({ data: ref(state.summary) }),
  useCollectionProductCounts: () => ({ ownership: ref(state.otherCounts) }),
}))

// A deterministic money formatter: USD passthrough, null-in → null-out (so the value stat hides).
vi.mock('@/composables/useCurrency', () => ({
  useCurrency: () => ({
    formatUsd: (raw: string | null | undefined) => (raw == null ? null : `$${raw}`),
  }),
}))

// A props-echoing ProductGrid stub lets the test assert both counts maps without
// deep-rendering the real tiles/images.
const ProductGridStub = {
  name: 'ProductGrid',
  props: ['game', 'products', 'wanted', 'owned'],
  template: '<div class="grid-stub" />',
}

function entry(id: string, quantity: number, foilQuantity = 0): ProductHoldingEntry {
  return {
    product: {
      id,
      name: `Product ${id}`,
      set_code: 'abc',
      set_name: 'ABC Set',
      product_type: 'bundle',
      url: null,
      has_image: false,
      prices: { usd: null, usd_foil: null },
      msrp: null,
      released_at: null,
    },
    quantity,
    foil_quantity: foilQuantity,
  }
}

function group(
  code: string,
  name: string | null,
  entries: ProductHoldingEntry[],
  opts: { uniqueProducts?: number; totalValueUsd?: string | null } = {},
): ProductHoldingSetGroup {
  return {
    code,
    name,
    unique_products: opts.uniqueProducts ?? entries.length,
    total_products: entries.reduce((sum, e) => sum + e.quantity + e.foil_quantity, 0),
    total_value_usd: opts.totalValueUsd ?? null,
    products: entries,
  }
}

// The page unit is a SET group, so `total` counts sets (not products); the by-set page size is 10.
function setPage(groups: ProductHoldingSetGroup[], total = groups.length): ProductHoldingSetPage {
  return { data: groups, page: 1, page_size: 10, total, has_more: total > 10 }
}

function summary(uniqueProducts: number): ProductHoldingSummary {
  return { unique_products: uniqueProducts, total_products: uniqueProducts, total_value_usd: null }
}

async function mountSection(path = '/wishlist/mtg', list: 'collection' | 'wishlist' = 'wishlist') {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/wishlist/:game', component: { template: '<div />' } },
      { path: '/collection/:game', component: { template: '<div />' } },
    ],
  })
  await router.push(path)

  const wrapper = mount(ProductHoldingSection, {
    props: { game: 'mtg', list },
    global: {
      plugins: [router],
      stubs: {
        RouterLink: RouterLinkStub,
        ProductGrid: ProductGridStub,
        CardPagination: true,
      },
    },
  })
  return { wrapper, router }
}

/** The set-group links (excludes the header's "Browse sealed" link, which carries no ?set=). */
function setLinks(wrapper: Awaited<ReturnType<typeof mountSection>>['wrapper']) {
  return wrapper
    .findAllComponents(RouterLinkStub)
    .filter((link) => String(link.props('to')).includes('?set='))
}

beforeEach(() => {
  state.data = undefined
  state.summary = undefined
  state.isPlaceholderData = false
  state.isSuccess = true
  state.page = undefined
  state.dataRef = undefined
  state.otherCounts = {}
})

describe('ProductHoldingSection', () => {
  it('renders nothing while data is undefined', async () => {
    state.data = undefined
    const { wrapper } = await mountSection()
    expect(wrapper.find('section').exists()).toBe(false)
  })

  it('renders nothing when the wish list has no sealed products', async () => {
    // Zero products ⇒ zero sets, so the set-count gate still hides the section.
    state.data = setPage([], 0)
    const { wrapper } = await mountSection()
    expect(wrapper.find('section').exists()).toBe(false)
  })

  it('renders the heading, the summary-sourced count, and the wanted map keyed by id', async () => {
    state.data = setPage([group('abc', 'ABC Set', [entry('100', 3), entry('200', 1, 2)])])
    // The header count is the unique-product tally from the summary, not the page (set) total.
    state.summary = summary(2)
    const { wrapper } = await mountSection()

    expect(wrapper.find('section').exists()).toBe(true)
    const heading = wrapper.get('h2').text()
    expect(heading).toContain('Sealed products')
    expect(heading).toContain('2 wanted')

    const grid = wrapper.getComponent(ProductGridStub)
    expect((grid.props('products') as unknown[]).length).toBe(2)

    // The wanted counts are passed as a map keyed by external product id (from page data).
    expect(grid.props('wanted')).toEqual({
      '100': { quantity: 3, foil_quantity: 0 },
      '200': { quantity: 1, foil_quantity: 2 },
    })
    expect(grid.props('owned')).toEqual({})
  })

  it('hides the header count span until the summary has data', async () => {
    state.data = setPage([group('abc', 'ABC Set', [entry('100', 1)])])
    state.summary = undefined
    const { wrapper } = await mountSection()
    // The section renders (a set is held) but the count span waits for the summary.
    expect(wrapper.find('section').exists()).toBe(true)
    expect(wrapper.get('h2').text()).not.toContain('wanted')
  })

  it('renders one group block per set in server order and links each to its sealed set view', async () => {
    state.data = setPage([
      group('aaa', 'Alpha', [entry('1', 1)]),
      group('bbb', 'Beta', [entry('2', 1)]),
      group('ccc', 'Gamma', [entry('3', 1)]),
    ])
    state.summary = summary(3)
    const { wrapper } = await mountSection('/collection/mtg', 'collection')

    // One ProductGrid per group, in server order.
    const grids = wrapper.findAllComponents(ProductGridStub)
    expect(grids.length).toBe(3)

    const links = setLinks(wrapper)
    expect(links.map((link) => link.text())).toEqual(['Alpha', 'Beta', 'Gamma'])
    expect(links.map((link) => link.props('to'))).toEqual([
      '/sealed/mtg?set=aaa',
      '/sealed/mtg?set=bbb',
      '/sealed/mtg?set=ccc',
    ])
  })

  it("shows each group's name, unique-product count, and value", async () => {
    state.data = setPage([
      group('abc', 'ABC Set', [entry('100', 3)], { uniqueProducts: 3, totalValueUsd: '12.50' }),
    ])
    state.summary = summary(3)
    const { wrapper } = await mountSection()

    const link = setLinks(wrapper)[0]!
    expect(link.text()).toBe('ABC Set')
    expect(link.props('to')).toBe('/sealed/mtg?set=abc')
    const header = (link.element.parentElement?.textContent ?? '').replace(/\s+/g, ' ').trim()
    expect(header).toContain('3 products')
    expect(header).toContain('$12.50')
  })

  it('falls back to the upper-cased code when the set has no catalog name', async () => {
    state.data = setPage([group('xyz', null, [entry('100', 1)], { totalValueUsd: null })])
    state.summary = summary(1)
    const { wrapper } = await mountSection()

    const link = setLinks(wrapper)[0]!
    expect(link.text()).toBe('XYZ')
    // A null value hides the value stat.
    const header = (link.element.parentElement?.textContent ?? '').replace(/\s+/g, ' ').trim()
    expect(header).not.toContain('$')
  })

  it('flattens the owned/wanted maps across every group into each grid', async () => {
    state.data = setPage([
      group('aaa', 'Alpha', [entry('100', 3)]),
      group('bbb', 'Beta', [entry('200', 1, 2)]),
    ])
    state.summary = summary(2)
    const { wrapper } = await mountSection()

    // Every group's grid gets the same map flattened across all groups (per-id lookup makes
    // the extra other-group keys harmless), so a cross-listed product never reads a false zero.
    const flattened = {
      '100': { quantity: 3, foil_quantity: 0 },
      '200': { quantity: 1, foil_quantity: 2 },
    }
    for (const grid of wrapper.findAllComponents(ProductGridStub)) {
      expect(grid.props('wanted')).toEqual(flattened)
      expect(grid.props('owned')).toEqual({})
    }
  })

  it('uses the same section for collection products and passes owned counts', async () => {
    state.data = setPage([group('abc', 'ABC Set', [entry('100', 2, 1)])])
    state.summary = summary(1)
    state.otherCounts = { '100': { quantity: 4, foil_quantity: 0 } }
    const { wrapper } = await mountSection('/collection/mtg', 'collection')

    expect(wrapper.get('h2').text()).toContain('1 owned')
    const grid = wrapper.getComponent(ProductGridStub)
    expect(grid.props('owned')).toEqual({
      '100': { quantity: 2, foil_quantity: 1 },
    })
    expect(grid.props('wanted')).toEqual({
      '100': { quantity: 4, foil_quantity: 0 },
    })
  })

  it('paginates only when the set total exceeds one page (10)', async () => {
    // At or under a page of sets: no pager.
    state.data = setPage([group('abc', 'ABC Set', [entry('100', 1)])], 5)
    expect((await mountSection()).wrapper.findComponent(CardPagination).exists()).toBe(false)

    // Over a page of sets: the pager renders.
    state.data = setPage([group('abc', 'ABC Set', [entry('100', 1)])], 25)
    expect((await mountSection()).wrapper.findComponent(CardPagination).exists()).toBe(true)
  })

  it('restores page 2 from the URL and clamps the URL when the set total shrinks', async () => {
    // Returning from a product keeps the wishlist's ?page=2 in history, so the section must
    // seed its query from that route rather than remounting at page 1.
    state.data = setPage([group('abc', 'ABC Set', [entry('100', 1)])], 25)
    const { router } = await mountSection('/wishlist/mtg?page=2')
    expect(state.page!.value).toBe(2)

    // If the sets on page 2 fall away (now a single page of sets), the shared clamp returns to
    // page 1 and canonicalizes it by dropping the query key. Clamp now counts in sets.
    state.dataRef!.value = setPage([], 10)
    await nextTick()
    await flushPromises()

    expect(state.page!.value).toBe(1)
    expect(router.currentRoute.value.query.page).toBeUndefined()
  })
})
