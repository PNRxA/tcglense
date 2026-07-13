import { describe, it, expect, beforeEach, vi } from 'vitest'
import { nextTick, ref, type Ref } from 'vue'
import { mount, RouterLinkStub } from '@vue/test-utils'
import type { WishlistProductEntry, WishlistProductPage } from '@/lib/api'
import WishlistSealedSection from '../WishlistSealedSection.vue'
import CardPagination from '@/components/cards/CardPagination.vue'

// Drive the section off controlled query state rather than the network: the wish-list
// products composable is mocked so the test exercises the section's render gating (self-hides
// when nothing is wanted), its header counts, its per-product badges, and the page-when-needed
// rule — not the query layer (covered by the API path + backend tests). `page` and `dataRef`
// capture the real refs handed to the mock on the most recent mount, so a test can mutate them
// after mounting to exercise clamp-page reactivity — the other fields are only read as seed
// values at mount time.
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

// A ProductGrid stub that renders the #badge scoped slot per product, so the section's
// OwnedCountBadge (itself stubbed to echo its counts) is exercised without deep-rendering
// the real tiles/images.
const ProductGridStub = {
  name: 'ProductGrid',
  props: ['game', 'products'],
  template: `<div class="grid-stub">
    <div v-for="p in products" :key="p.id" class="tile" :data-id="p.id">
      <slot name="badge" :product="p" />
    </div>
  </div>`,
}
const BadgeStub = {
  name: 'OwnedCountBadge',
  props: ['quantity', 'foilQuantity'],
  template: '<span class="badge">{{ quantity }}/{{ foilQuantity }}</span>',
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

function mountSection() {
  return mount(WishlistSealedSection, {
    props: { game: 'mtg' },
    global: {
      stubs: {
        RouterLink: RouterLinkStub,
        ProductGrid: ProductGridStub,
        OwnedCountBadge: BadgeStub,
        CardPagination: true,
      },
    },
  })
}

beforeEach(() => {
  state.data = undefined
  state.isPlaceholderData = false
  state.isSuccess = true
  state.page = undefined
  state.dataRef = undefined
})

describe('WishlistSealedSection', () => {
  it('renders nothing while data is undefined', () => {
    state.data = undefined
    const wrapper = mountSection()
    expect(wrapper.find('section').exists()).toBe(false)
  })

  it('renders nothing when the wish list has no sealed products', () => {
    state.data = pageData([], 0)
    const wrapper = mountSection()
    expect(wrapper.find('section').exists()).toBe(false)
  })

  it('renders the heading, total, tiles, and per-product badge counts', () => {
    state.data = pageData([entry('100', 3), entry('200', 1, 2)])
    const wrapper = mountSection()

    expect(wrapper.find('section').exists()).toBe(true)
    const heading = wrapper.get('h2').text()
    expect(heading).toContain('Sealed products')
    expect(heading).toContain('2 wanted')

    const grid = wrapper.getComponent(ProductGridStub)
    expect((grid.props('products') as unknown[]).length).toBe(2)

    // Badges carry the wanted counts keyed by product id.
    const badges = wrapper.findAll('.badge').map((b) => b.text())
    expect(badges).toEqual(['3/0', '1/2'])
  })

  it('paginates only when the total exceeds one page (60)', () => {
    // Under a page: no pager.
    state.data = pageData([entry('100', 1)], 2)
    expect(mountSection().findComponent(CardPagination).exists()).toBe(false)

    // Over a page: the pager renders.
    state.data = pageData([entry('100', 1)], 100)
    expect(mountSection().findComponent(CardPagination).exists()).toBe(true)
  })

  it('clamps back to page 1 once a shrunk total no longer reaches the current page', async () => {
    // 100 wanted, 60 per page: page 2 is valid at mount.
    state.data = pageData([entry('100', 1)], 100)
    mountSection()

    // The user is sitting on page 2 (as if the pager had carried them there)...
    state.page!.value = 2

    // ...then the quick-add dialog zeroes the last product on that page, and the refetch at
    // page=2 comes back with a total that no longer reaches a second page.
    state.dataRef!.value = pageData([], 60)
    await nextTick()

    expect(state.page!.value).toBe(1)
  })
})
