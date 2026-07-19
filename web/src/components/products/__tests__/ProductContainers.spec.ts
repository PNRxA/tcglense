import { describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Product, ProductContainer } from '@/lib/api'
import ProductContainers from '../ProductContainers.vue'

// Drive the "Included in" section off a controlled parent list, stubbing the query composable.
// The unit under test is which parents render, their contained quantity, and that a plain click
// opens the shared sealed-product modal in place (issue #485) rather than navigating away.
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

async function mountContainers(containers: ProductContainer[], path = '/sealed/mtg/booster-pack') {
  state.containers = containers
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/sealed/:game', component: { template: '<div />' } },
      { path: '/sealed/:game/:id', component: { template: '<div />' } },
    ],
  })
  await router.push(path)
  await router.isReady()
  const wrapper = mount(ProductContainers, {
    props: { game: 'mtg', id: 'booster-pack' },
    global: { plugins: [router] },
  })
  return { wrapper, router }
}

describe('ProductContainers', () => {
  it('renders nothing when no product contains the viewed item', async () => {
    const { wrapper } = await mountContainers([])
    expect(wrapper.find('h2').exists()).toBe(false)
    expect(wrapper.findAll('a')).toHaveLength(0)
  })

  it('lists parent products with the contained quantity and a canonical href', async () => {
    const { wrapper } = await mountContainers([
      { product: product('box', 'Play Booster Box'), quantity: 36 },
      { product: product('bundle', 'Gift Bundle'), quantity: 9 },
    ])

    expect(wrapper.find('h2').text()).toContain('Included in')
    expect(wrapper.find('h2').text()).toContain('2 products')
    const links = wrapper.findAll('a')
    expect(links).toHaveLength(2)
    expect(links[0]!.text()).toContain('Play Booster Box')
    expect(links[0]!.text()).toContain('Contains 36× this product')
    // The anchor keeps the canonical product page as its href for modifier/middle clicks + crawlers.
    expect(links[0]!.attributes('href')).toBe('/sealed/mtg/box')
    expect(links[1]!.text()).toContain('Contains 9× this product')
    expect(links[1]!.attributes('href')).toBe('/sealed/mtg/bundle')
  })

  it('opens the parent in the sealed-product modal in place, keeping the page', async () => {
    const { wrapper, router } = await mountContainers([
      { product: product('box', 'Play Booster Box'), quantity: 36 },
    ])
    await wrapper.get('a').trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/sealed/mtg/booster-pack')
    expect(router.currentRoute.value.query.product).toBe('box')
  })

  it('leaves modifier-click navigation to the browser', async () => {
    const { wrapper, router } = await mountContainers([
      { product: product('box', 'Play Booster Box'), quantity: 36 },
    ])
    const push = vi.spyOn(router, 'push')
    wrapper.get('a').element.addEventListener('click', (event) => event.preventDefault(), {
      once: true,
    })
    await wrapper.get('a').trigger('click', { metaKey: true })
    expect(push).not.toHaveBeenCalled()
  })

  it('uses the package fallback when a parent has no image', async () => {
    const { wrapper } = await mountContainers([
      { product: product('with-art', 'With art', true), quantity: 1 },
      { product: product('without-art', 'Without art', false), quantity: 1 },
    ])
    const items = wrapper.findAll('li')
    expect(items[0]!.find('img').exists()).toBe(true)
    expect(items[1]!.find('img').exists()).toBe(false)
  })
})
