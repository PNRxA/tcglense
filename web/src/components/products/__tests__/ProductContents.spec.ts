import { describe, it, expect, vi } from 'vitest'
import { mount, RouterLinkStub } from '@vue/test-utils'
import type { Card, Product, ProductComponent } from '@/lib/api'
import ProductContents from '../ProductContents.vue'

// Drive the component off a controlled composition, stubbing the query composable so no
// QueryClient / router is needed — the unit under test is which line items render, their
// quantity + name, and which ones link where (issue: sealed-product composition).
const state = vi.hoisted(() => ({ components: [] as ProductComponent[] }))

vi.mock('@/composables/useProducts', () => ({
  useProductContentsQuery: () => ({
    data: {
      get value() {
        return { data: state.components }
      },
    },
  }),
}))

// Minimal component/product/card factories carrying only the fields the template reads.
function comp(overrides: Partial<ProductComponent> = {}): ProductComponent {
  return { kind: 'other', name: 'Extra', quantity: 1, product: null, card: null, ...overrides }
}
function linkedProduct(id: string, hasImage = true): Product {
  return { id, name: `Product ${id}`, has_image: hasImage } as unknown as Product
}
function linkedCard(id: string, hasImage = false): Card {
  return { id, name: `Card ${id}`, has_image: hasImage } as unknown as Card
}

function mountContents(components: ProductComponent[]) {
  state.components = components
  return mount(ProductContents, {
    props: { game: 'mtg', id: '900003' },
    global: { stubs: { RouterLink: RouterLinkStub } },
  })
}

describe('ProductContents', () => {
  it('renders nothing when the product has no composition', () => {
    const wrapper = mountContents([])
    expect(wrapper.find('h2').exists()).toBe(false)
    expect(wrapper.findAllComponents(RouterLinkStub)).toHaveLength(0)
  })

  it('lists each component with its quantity and name, in order', () => {
    const wrapper = mountContents([
      comp({ kind: 'sealed', name: 'Play Booster', quantity: 9, product: linkedProduct('648640') }),
      comp({ kind: 'other', name: 'Card storage box', quantity: 1 }),
    ])
    // Heading carries the section title plus a live item count (SEO enrichment, issue #302).
    expect(wrapper.find('h2').text()).toContain("What's in the box")
    expect(wrapper.find('h2').text()).toContain('2 items')
    const items = wrapper.findAll('li')
    expect(items).toHaveLength(2)
    expect(items[0]!.text()).toContain('9×')
    expect(items[0]!.text()).toContain('Play Booster')
    expect(items[1]!.text()).toContain('Card storage box')
  })

  it('links sealed components to the product page and card components to the card page', () => {
    const wrapper = mountContents([
      comp({ kind: 'sealed', name: 'Play Booster', quantity: 9, product: linkedProduct('648640') }),
      comp({ kind: 'card', name: 'Sol Ring', quantity: 1, card: linkedCard('sf-sol') }),
      comp({ kind: 'other', name: 'Storage box', quantity: 1 }),
    ])
    const links = wrapper.findAllComponents(RouterLinkStub)
    // Only the two resolvable components link; the textual extra does not.
    expect(links).toHaveLength(2)
    expect(links[0]!.props('to')).toEqual({
      name: 'sealed-product',
      params: { game: 'mtg', id: '648640' },
    })
    expect(links[1]!.props('to')).toEqual({
      name: 'card',
      params: { game: 'mtg', id: 'sf-sol' },
    })
  })

  it('shows a thumbnail only for links with art, else the icon fallback', () => {
    const wrapper = mountContents([
      comp({ kind: 'sealed', name: 'With art', quantity: 1, product: linkedProduct('1', true) }),
      comp({ kind: 'sealed', name: 'No art', quantity: 1, product: linkedProduct('2', false) }),
      comp({ kind: 'card', name: 'Card art', quantity: 1, card: linkedCard('c1', true) }),
      comp({ kind: 'card', name: 'Card no art', quantity: 1, card: linkedCard('c2', false) }),
      comp({ kind: 'other', name: 'Die', quantity: 1 }),
    ])
    const items = wrapper.findAll('li')
    // Only the art-bearing product + card render an <img>; art-less links + the textual
    // extra fall back to the kind icon (never a broken image).
    expect(items[0]!.find('img').exists()).toBe(true)
    expect(items[1]!.find('img').exists()).toBe(false)
    expect(items[2]!.find('img').exists()).toBe(true)
    expect(items[3]!.find('img').exists()).toBe(false)
    expect(items[4]!.find('img').exists()).toBe(false)
  })
})
