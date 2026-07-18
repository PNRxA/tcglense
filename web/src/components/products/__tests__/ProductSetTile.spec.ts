import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { CardSet, ProductHoldingSet } from '@/lib/api'
import { makeCardSet } from '@/test/fixtures'
import ProductSetTile from '../ProductSetTile.vue'

const makeProductSet = (over: Partial<ProductHoldingSet> = {}): ProductHoldingSet => ({
  code: 'blb',
  name: 'Bloomburrow',
  unique_products: 3,
  total_products: 3,
  total_value_usd: null,
  ...over,
})

// ProductSetTile renders a RouterLink and warms the route on hover, so the tree needs a router;
// no catalog icon keeps the lazy <img> off, so nothing network-facing is exercised by default.
function mountTile(props: { set: ProductHoldingSet; catalogSet?: CardSet; value?: string | null }) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
  })
  return mount(ProductSetTile, {
    props: { game: 'mtg', to: '/collection/mtg/products/sets/blb', ...props },
    global: { plugins: [router] },
  })
}

describe('ProductSetTile', () => {
  it('shows the set name and distinct-product count', () => {
    const wrapper = mountTile({ set: makeProductSet({ unique_products: 3 }) })
    expect(wrapper.get('p.font-medium').text()).toBe('Bloomburrow')
    expect(wrapper.text()).toContain('3 products')
  })

  it('reads "1 product" (singular) for a single held product', () => {
    const wrapper = mountTile({ set: makeProductSet({ unique_products: 1, total_products: 1 }) })
    expect(wrapper.text()).toContain('1 product')
    expect(wrapper.text()).not.toContain('1 products')
  })

  it('falls back to the upper-cased code when the set has no catalog name', () => {
    const wrapper = mountTile({ set: makeProductSet({ code: 'xyz', name: null }) })
    expect(wrapper.get('p.font-medium').text()).toBe('XYZ')
  })

  it('appends "N copies" only when there are more copies than distinct products', () => {
    const wrapper = mountTile({ set: makeProductSet({ unique_products: 3, total_products: 5 }) })
    expect(wrapper.text()).toContain('5 copies')
  })

  it('omits the copies line when every held product is a single copy', () => {
    const wrapper = mountTile({ set: makeProductSet({ unique_products: 3, total_products: 3 }) })
    expect(wrapper.text()).not.toContain('copies')
  })

  it('pins the total value labelled "Total" only when a value is passed', () => {
    const withValue = mountTile({ set: makeProductSet(), value: '$42.00' })
    expect(withValue.text()).toContain('Total')
    expect(withValue.text()).toContain('$42.00')

    const withoutValue = mountTile({ set: makeProductSet(), value: null })
    expect(withoutValue.text()).not.toContain('Total')
    expect(withoutValue.text()).not.toContain('$')
  })

  it('renders the release date from the catalog set when one is provided', () => {
    const wrapper = mountTile({
      set: makeProductSet(),
      catalogSet: makeCardSet('blb', { released_at: '2024-08-02', icon_svg_uri: null }),
    })
    // Formatted "Mon YYYY" (locale-dependent month, so assert on the year).
    expect(wrapper.text()).toContain('2024')
  })

  it('shows the Package fallback icon (no <img>) when there is no catalog set', () => {
    const wrapper = mountTile({ set: makeProductSet() })
    expect(wrapper.find('img').exists()).toBe(false)
  })

  it('renders the set icon through the proxy when the catalog set has one', () => {
    const wrapper = mountTile({
      set: makeProductSet(),
      catalogSet: makeCardSet('blb', { icon_svg_uri: 'https://example.test/blb.svg' }),
    })
    const img = wrapper.find('img')
    expect(img.exists()).toBe(true)
    expect(img.attributes('src')).toContain('/sets/blb/icon')
  })
})
