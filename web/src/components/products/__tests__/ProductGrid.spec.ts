import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { OwnedCountsMap, Product } from '@/lib/api'
import ProductGrid from '../ProductGrid.vue'
import { useAuthStore } from '@/stores/auth'

function makeProduct(id: string): Product {
  return {
    id,
    name: `Product ${id}`,
    set_code: 'tst',
    set_name: 'TST',
    product_type: 'bundle',
    url: null,
    has_image: false,
    prices: { usd: null, usd_foil: null },
    msrp: null,
    released_at: null,
  }
}

// A props-echoing stub so the test can assert what counts each tile's unified control receives,
// without deep-rendering the real popover/editor (covered by ProductCountControl.spec).
const ProductControlStub = {
  name: 'ProductCountControl',
  props: ['game', 'productId', 'name', 'quantity', 'foilQuantity', 'wishlistQuantity'],
  template:
    '<div class="wanted-stub" :data-id="productId" :data-qty="quantity" :data-foil="foilQuantity" :data-wanted="wishlistQuantity" />',
}

// Signed in unless `authenticated: false` — the quick-add controls only render for a
// signed-in user (CardGrid parity).
function mountGrid(
  products: Product[],
  wanted?: OwnedCountsMap,
  authenticated = true,
  owned?: OwnedCountsMap,
) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/sealed/:game/:id', component: { template: '<div />' } }],
  })
  const pinia = createPinia()
  setActivePinia(pinia)
  if (authenticated) useAuthStore().accessToken = 'test-token'
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return mount(ProductGrid, {
    props: { game: 'mtg', products, wanted, owned },
    global: {
      plugins: [router, pinia, [VueQueryPlugin, { queryClient }]],
      stubs: { ProductCountControl: ProductControlStub, ProductImage: true },
    },
  })
}

function controls(wrapper: ReturnType<typeof mountGrid>) {
  return wrapper.findAll('.wanted-stub')
}

describe('ProductGrid unified quick-add controls', () => {
  it('passes each product its combined wanted total, zero when absent from the map', () => {
    const wrapper = mountGrid([makeProduct('100'), makeProduct('200')], {
      '100': { quantity: 3, foil_quantity: 1 },
    })
    const stubs = controls(wrapper)
    expect(stubs).toHaveLength(2)

    const first = stubs.find((control) => control.attributes('data-id') === '100')!
    expect(first.attributes('data-wanted')).toBe('4')

    // A product missing from the map rests at zero.
    const second = stubs.find((control) => control.attributes('data-id') === '200')!
    expect(second.attributes('data-wanted')).toBe('0')
  })

  it('renders no controls for a signed-out visitor but still renders the tile links', () => {
    const wrapper = mountGrid([makeProduct('100'), makeProduct('200')], undefined, false)
    expect(controls(wrapper)).toHaveLength(0)
    expect(wrapper.find('a[href="/sealed/mtg/100"]').exists()).toBe(true)
    expect(wrapper.find('a[href="/sealed/mtg/200"]').exists()).toBe(true)
  })

  it('renders controls at zero counts when no wanted map is given', () => {
    const wrapper = mountGrid([makeProduct('100')])
    const stubs = controls(wrapper)
    expect(stubs).toHaveLength(1)
    expect(stubs[0]!.attributes('data-qty')).toBe('0')
    expect(stubs[0]!.attributes('data-foil')).toBe('0')
    expect(stubs[0]!.attributes('data-wanted')).toBe('0')
  })

  it('passes collection counts to the collection-primary control', () => {
    const wrapper = mountGrid([makeProduct('100')], undefined, true, {
      '100': { quantity: 2, foil_quantity: 1 },
    })
    const owned = controls(wrapper)[0]!
    expect(owned.attributes('data-qty')).toBe('2')
    expect(owned.attributes('data-foil')).toBe('1')
  })
})
