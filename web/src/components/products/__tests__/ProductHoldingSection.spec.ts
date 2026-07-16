import { describe, it, expect, beforeEach, vi } from 'vitest'
import { nextTick, ref, type Ref } from 'vue'
import { flushPromises, mount, RouterLinkStub } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { ProductHoldingEntry, ProductHoldingPage } from '@/lib/api'
import ProductHoldingSection from '@/components/products/ProductHoldingSection.vue'
import CardPagination from '@/components/cards/CardPagination.vue'

// Drive the section off controlled query state rather than the network: both product
// composables are mocked so the test exercises render gating, header counts, both holding
// maps handed to the grid, and the page-when-needed rule — not the query layer (covered by
// the API path + backend tests).
// `page` and `dataRef` capture the real refs handed to the mock on the most recent mount, so a
// test can mutate them after mounting to exercise clamp-page reactivity — the other fields are
// only read as seed values at mount time.
const state = vi.hoisted(() => ({
  data: undefined as ProductHoldingPage | undefined,
  isPlaceholderData: false,
  isSuccess: true,
  page: undefined as Ref<number> | undefined,
  dataRef: undefined as Ref<ProductHoldingPage | undefined> | undefined,
  otherCounts: {} as Record<string, { quantity: number; foil_quantity: number }>,
}))

vi.mock('@/composables/useWishlist', () => ({
  WISHLIST_PRODUCT_PAGE_SIZE: 60,
  useWishlistProductsQuery: (_game: unknown, page: Ref<number>) => {
    state.page = page
    const dataRef = ref(state.data) as Ref<ProductHoldingPage | undefined>
    state.dataRef = dataRef
    return {
      data: dataRef,
      isPlaceholderData: ref(state.isPlaceholderData),
      isSuccess: ref(state.isSuccess),
    }
  },
  useWishlistProductCounts: () => ({ ownership: ref(state.otherCounts) }),
}))

vi.mock('@/composables/useCollection', () => ({
  COLLECTION_PRODUCT_PAGE_SIZE: 60,
  useCollectionProductsQuery: (_game: unknown, page: Ref<number>) => {
    state.page = page
    const dataRef = ref(state.data) as Ref<ProductHoldingPage | undefined>
    state.dataRef = dataRef
    return {
      data: dataRef,
      isPlaceholderData: ref(state.isPlaceholderData),
      isSuccess: ref(state.isSuccess),
    }
  },
  useCollectionProductCounts: () => ({ ownership: ref(state.otherCounts) }),
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

function pageData(entries: ProductHoldingEntry[], total = entries.length): ProductHoldingPage {
  return { data: entries, page: 1, page_size: 60, total, has_more: total > 60 }
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

beforeEach(() => {
  state.data = undefined
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
    state.data = pageData([], 0)
    const { wrapper } = await mountSection()
    expect(wrapper.find('section').exists()).toBe(false)
  })

  it('renders the heading, total, tiles, and passes the wanted counts map keyed by id', async () => {
    state.data = pageData([entry('100', 3), entry('200', 1, 2)])
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

  it('paginates only when the total exceeds one page (60)', async () => {
    // Under a page: no pager.
    state.data = pageData([entry('100', 1)], 2)
    expect((await mountSection()).wrapper.findComponent(CardPagination).exists()).toBe(false)

    // Over a page: the pager renders.
    state.data = pageData([entry('100', 1)], 100)
    expect((await mountSection()).wrapper.findComponent(CardPagination).exists()).toBe(true)
  })

  it('uses the same section for collection products and passes owned counts', async () => {
    state.data = pageData([entry('100', 2, 1)])
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

  it('restores page 2 from the URL and clamps the URL when the total shrinks', async () => {
    // Returning from a product keeps the wishlist's ?page=2 in history, so the section
    // must seed its query from that route rather than remounting at page 1.
    state.data = pageData([entry('100', 1)], 100)
    const { router } = await mountSection('/wishlist/mtg?page=2')
    expect(state.page!.value).toBe(2)

    // If the last product on page 2 is removed, the shared clamp returns to page 1 and
    // canonicalizes it by dropping the query key.
    state.dataRef!.value = pageData([], 60)
    await nextTick()
    await flushPromises()

    expect(state.page!.value).toBe(1)
    expect(router.currentRoute.value.query.page).toBeUndefined()
  })
})
