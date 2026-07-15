import { describe, expect, it, vi } from 'vitest'
import { mount, RouterLinkStub } from '@vue/test-utils'
import type { Product, ProductContainer } from '@/lib/api'
import ProductContainers from '../ProductContainers.vue'

const state = vi.hoisted(() => ({ containers: [] as ProductContainer[] }))

vi.mock('@/composables/useProducts', () => ({
  useProductContainersQuery: () => ({
    data: {
      get value() {
        return { data: state.containers }
      },
    },
  }),
}))

function product(id: string, name: string, hasImage = true): Product {
  return {
    id,
    name,
    product_type: 'play_display',
    has_image: hasImage,
  } as Product
}

function mountContainers(containers: ProductContainer[]) {
  state.containers = containers
  return mount(ProductContainers, {
    props: { game: 'mtg', id: 'booster-pack' },
    global: { stubs: { RouterLink: RouterLinkStub } },
  })
}

describe('ProductContainers', () => {
  it('renders nothing when no product contains the viewed item', () => {
    const wrapper = mountContainers([])
    expect(wrapper.find('h2').exists()).toBe(false)
    expect(wrapper.findAllComponents(RouterLinkStub)).toHaveLength(0)
  })

  it('lists parent products with the contained quantity and links to their detail pages', () => {
    const wrapper = mountContainers([
      { product: product('box', 'Play Booster Box'), quantity: 36 },
      { product: product('bundle', 'Gift Bundle'), quantity: 9 },
    ])

    expect(wrapper.find('h2').text()).toContain('Included in')
    expect(wrapper.find('h2').text()).toContain('2 products')
    const links = wrapper.findAllComponents(RouterLinkStub)
    expect(links).toHaveLength(2)
    expect(links[0]!.text()).toContain('Play Booster Box')
    expect(links[0]!.text()).toContain('Contains 36× this product')
    expect(links[0]!.props('to')).toEqual({
      name: 'sealed-product',
      params: { game: 'mtg', id: 'box' },
    })
    expect(links[1]!.text()).toContain('Contains 9× this product')
  })

  it('uses the package fallback when a parent has no image', () => {
    const wrapper = mountContainers([
      { product: product('with-art', 'With art', true), quantity: 1 },
      { product: product('without-art', 'Without art', false), quantity: 1 },
    ])
    const items = wrapper.findAll('li')
    expect(items[0]!.find('img').exists()).toBe(true)
    expect(items[1]!.find('img').exists()).toBe(false)
  })
})
