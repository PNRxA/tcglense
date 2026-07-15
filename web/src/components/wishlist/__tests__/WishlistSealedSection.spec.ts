import { describe, it, expect, beforeEach, vi } from 'vitest'
import { nextTick, ref, type Ref } from 'vue'
import { flushPromises, mount, RouterLinkStub } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { WishlistProductEntry, WishlistProductPage } from '@/lib/api'
import WishlistSealedSection from '../WishlistSealedSection.vue'
import CardPagination from '@/components/cards/CardPagination.vue'

// Drive the section off controlled query state rather than the network: the wish-list
// products composable is mocked so the test exercises the section's render gating (self-hides
// when nothing is wanted), its header counts, the wanted-count map it hands the grid, and the
// page-when-needed rule — not the query layer (covered by the API path + backend tests).
// `page` and `dataRef` capture the real refs handed to the mock on the most recent mount, so a
// test can mutate them after mounting to exercise clamp-page reactivity — the other fields are
// only read as seed values at mount time.
const state = vi.hoisted(() => ({
  data: undefined as WishlistProductPage | undefined,
  isPlaceholderData: false,
  isSuccess: true,
  page: undefined as Ref<number> | undefined,
  dataRef: undefined as Ref<WishlistProductPage | undefined> | undefined,
}))

vi.mock('@/composables/useWishlist', () => ({
  WISHLIST_PRODUCT_PAGE_SIZE: 60,
  useWishlistProductsQuery: (_game: unknown, page: Ref<number>) => {
    state.page = page
    const dataRef = ref(state.data) as Ref<WishlistProductPage | undefined>
    state.dataRef = dataRef
    return {
      data: dataRef,
      isPlaceholderData: ref(state.isPlaceholderData),
      isSuccess: ref(state.isSuccess),
    }
  },
}))

// A props-echoing ProductGrid stub: the section now hands the grid a `wanted` counts map
// (keyed by product id) instead of a #badge slot, so the test asserts on the stub's props
// without deep-rendering the real tiles/images.
const ProductGridStub = {
  name: 'ProductGrid',
  props: ['game', 'products', 'wanted'],
  template: '<div class="grid-stub" />',
}

function entry(id: string, quantity: number, foilQuantity = 0): WishlistProductEntry {
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

function pageData(entries: WishlistProductEntry[], total = entries.length): WishlistProductPage {
  return { data: entries, page: 1, page_size: 60, total, has_more: total > 60 }
}

async function mountSection(path = '/wishlist/mtg') {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/wishlist/:game', component: { template: '<div />' } }],
  })
  await router.push(path)

  const wrapper = mount(WishlistSealedSection, {
    props: { game: 'mtg' },
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
})

describe('WishlistSealedSection', () => {
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
  })

  it('paginates only when the total exceeds one page (60)', async () => {
    // Under a page: no pager.
    state.data = pageData([entry('100', 1)], 2)
    expect((await mountSection()).wrapper.findComponent(CardPagination).exists()).toBe(false)

    // Over a page: the pager renders.
    state.data = pageData([entry('100', 1)], 100)
    expect((await mountSection()).wrapper.findComponent(CardPagination).exists()).toBe(true)
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
