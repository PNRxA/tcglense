import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import type { Product } from '@/lib/api'
import ProductBuyLinks from '../ProductBuyLinks.vue'

// A sealed product with just the fields the buy-links builder reads set to
// interesting values; the rest are filled in with valid defaults.
function makeProduct(overrides: Partial<Product> = {}): Product {
  return {
    id: '900001',
    name: 'Bloomburrow Collector Booster Box',
    set_code: 'blb',
    set_name: 'Bloomburrow',
    product_type: 'collector_display',
    url: 'https://www.tcgplayer.com/product/517079',
    has_image: false,
    prices: { usd: '199.99', usd_foil: null },
    released_at: '2024-08-02',
    ...overrides,
  }
}

function anchors(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAll('a')
}

describe('ProductBuyLinks', () => {
  it('renders the "Where to buy" card with US and Australia sections for mtg', () => {
    const wrapper = mount(ProductBuyLinks, { props: { game: 'mtg', product: makeProduct() } })
    expect(wrapper.text()).toContain('Where to buy')
    const headings = wrapper.findAll('h3').map((h) => h.text())
    expect(headings).toEqual(['US', 'Australia'])
  })

  it('renders an outbound store link per store, each opening safely in a new tab', () => {
    const wrapper = mount(ProductBuyLinks, { props: { game: 'mtg', product: makeProduct() } })
    const links = anchors(wrapper)
    // 6 US + 9 Australia stores.
    expect(links).toHaveLength(15)
    for (const a of links) {
      expect(a.attributes('href')).toMatch(/^https:\/\//)
      expect(a.attributes('target')).toBe('_blank')
      expect(a.attributes('rel')).toContain('noopener')
    }
    const names = links.map((a) => a.text())
    expect(names).toContain('Amazon')
    expect(names).toContain('Good Games')
  })

  it('deep-links TCGplayer to the exact product page when a url is present', () => {
    const wrapper = mount(ProductBuyLinks, { props: { game: 'mtg', product: makeProduct() } })
    const tcg = anchors(wrapper).find((a) => a.text() === 'TCGplayer')
    expect(tcg?.attributes('href')).toBe('https://www.tcgplayer.com/product/517079')
  })

  it('falls back to a TCGplayer name search when the product carries no url', () => {
    const product = makeProduct({ name: 'Foundations Bundle', url: null })
    const wrapper = mount(ProductBuyLinks, { props: { game: 'mtg', product } })
    const tcg = anchors(wrapper).find((a) => a.text() === 'TCGplayer')
    expect(tcg?.attributes('href')).toContain('tcgplayer.com/search')
    expect(tcg?.attributes('href')).toContain(encodeURIComponent('Foundations Bundle'))
  })

  it('renders nothing for a game with no store registry', () => {
    const wrapper = mount(ProductBuyLinks, { props: { game: 'pokemon', product: makeProduct() } })
    expect(wrapper.find('a').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('Where to buy')
  })
})
