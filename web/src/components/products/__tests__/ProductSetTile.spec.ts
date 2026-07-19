import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { CardSet } from '@/lib/api'
import { makeCardSet } from '@/test/fixtures'
import ProductSetTile from '../ProductSetTile.vue'

// ProductSetTile renders a RouterLink and warms the route on hover, so the tree needs a router;
// no catalog icon keeps the lazy <img> off, so nothing network-facing is exercised by default.
// The tile is generalized over two call sites: the holdings section passes `copies`/`value`, the
// public catalog landing passes only `products` — so the props are supplied explicitly here.
function mountTile(props: {
  code?: string
  name?: string | null
  products: number
  copies?: number
  value?: string | null
  catalogSet?: CardSet
}) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
  })
  return mount(ProductSetTile, {
    props: {
      game: 'mtg',
      to: '/sealed/mtg/sets/blb',
      code: 'blb',
      name: 'Bloomburrow',
      ...props,
    },
    global: { plugins: [router] },
  })
}

describe('ProductSetTile', () => {
  it('shows the set name and product count', () => {
    const wrapper = mountTile({ products: 3 })
    expect(wrapper.get('p.font-medium').text()).toBe('Bloomburrow')
    expect(wrapper.text()).toContain('3 products')
  })

  it('reads "1 product" (singular) for a single product', () => {
    const wrapper = mountTile({ products: 1 })
    expect(wrapper.text()).toContain('1 product')
    expect(wrapper.text()).not.toContain('1 products')
  })

  it('falls back to the upper-cased code when the set has no catalog name', () => {
    const wrapper = mountTile({ code: 'xyz', name: null, products: 1 })
    expect(wrapper.get('p.font-medium').text()).toBe('XYZ')
  })

  it('appends "N copies" only when there are more copies than distinct products', () => {
    const wrapper = mountTile({ products: 3, copies: 5 })
    expect(wrapper.text()).toContain('5 copies')
  })

  it('omits the copies line when every product is a single copy', () => {
    const wrapper = mountTile({ products: 3, copies: 3 })
    expect(wrapper.text()).not.toContain('copies')
  })

  it('omits the copies line entirely in catalog mode (no copies prop)', () => {
    const wrapper = mountTile({ products: 3 })
    expect(wrapper.text()).not.toContain('copies')
  })

  it('pins the total value labelled "Total" only when a value is passed', () => {
    const withValue = mountTile({ products: 3, value: '$42.00' })
    expect(withValue.text()).toContain('Total')
    expect(withValue.text()).toContain('$42.00')

    const withoutValue = mountTile({ products: 3, value: null })
    expect(withoutValue.text()).not.toContain('Total')
    expect(withoutValue.text()).not.toContain('$')

    // Catalog mode omits `value` altogether — no value stat.
    const catalog = mountTile({ products: 3 })
    expect(catalog.text()).not.toContain('Total')
  })

  it('renders the release date from the catalog set when one is provided', () => {
    const wrapper = mountTile({
      products: 3,
      catalogSet: makeCardSet('blb', { released_at: '2024-08-02', icon_svg_uri: null }),
    })
    // Formatted "Mon YYYY" (locale-dependent month, so assert on the year).
    expect(wrapper.text()).toContain('2024')
  })

  it('shows the Package fallback icon (no <img>) when there is no catalog set', () => {
    const wrapper = mountTile({ products: 3 })
    expect(wrapper.find('img').exists()).toBe(false)
  })

  it('renders the set icon through the proxy when the catalog set has one', () => {
    const wrapper = mountTile({
      products: 3,
      catalogSet: makeCardSet('blb', { icon_svg_uri: 'https://example.test/blb.svg' }),
    })
    const img = wrapper.find('img')
    expect(img.exists()).toBe(true)
    expect(img.attributes('src')).toContain('/sets/blb/icon')
  })
})
