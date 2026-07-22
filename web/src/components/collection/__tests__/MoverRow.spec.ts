import { describe, expect, it } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Card, CollectionMover, CollectionSealedMover, Product } from '@/lib/api'
import MoverRow from '../MoverRow.vue'

function makeCard(): Card {
  return {
    id: 'card-1',
    name: 'Test Card',
    set_code: 'tst',
    set_name: 'Test Set',
    collector_number: '42',
    rarity: 'rare',
    lang: 'en',
    released_at: '2024-01-01',
    mana_cost: null,
    cmc: null,
    type_line: null,
    oracle_text: null,
    power: null,
    toughness: null,
    loyalty: null,
    color_identity: [],
    colors: [],
    layout: 'normal',
    prices: { usd: null, usd_foil: null, eur: null, tix: null },
    has_image: false,
    drop_name: null,
    drop_slug: null,
    secret_lair_bonus: false,
    secret_lair_spend_incentive: false,
    faces: [],
    legalities: null,
  }
}

function makeProduct(): Product {
  return {
    id: 'product-1',
    name: 'Test Bundle',
    set_code: 'tst',
    set_name: 'Test Set',
    product_type: 'bundle',
    url: null,
    has_image: false,
    prices: { usd: null, usd_foil: null },
    msrp: null,
    released_at: '2024-01-01',
  }
}

const CardImageStub = {
  name: 'CardImage',
  template: '<div class="card-image-stub" />',
}
const ProductImageStub = {
  name: 'ProductImage',
  template: '<div class="product-image-stub" />',
}

async function mountRow(mover: CollectionMover | CollectionSealedMover) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/collection/:game', component: { template: '<div />' } },
      { path: '/cards/:game/cards/:id', component: { template: '<div />' } },
      { path: '/sealed/:game/:id', component: { template: '<div />' } },
    ],
  })
  await router.push('/collection/mtg')
  await router.isReady()
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = mount(MoverRow, {
    props: { game: 'mtg', mover },
    global: {
      plugins: [router, createPinia(), [VueQueryPlugin, { queryClient }]],
      stubs: { CardImage: CardImageStub, ProductImage: ProductImageStub },
    },
  })
  return { router, wrapper }
}

describe('MoverRow mixed catalog items', () => {
  it('renders a sealed-product mover and navigates to its detail page', async () => {
    const product = makeProduct()
    const { router, wrapper } = await mountRow({
      product,
      quantity: 2,
      foil_quantity: 0,
      value_now: '60.00',
      value_prev: '40.00',
      change_usd: '20.00',
      change_pct: 50,
    })

    expect(wrapper.find('.product-image-stub').exists()).toBe(true)
    expect(wrapper.find('.card-image-stub').exists()).toBe(false)
    expect(wrapper.text()).toContain('Test Bundle')
    expect(wrapper.text()).toContain('Test Set · Bundle')
    // The owned count is surfaced (2 copies, no foils → total chip only).
    expect(wrapper.find('[aria-label="2 owned"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label$="foil"]').exists()).toBe(false)
    expect(wrapper.get('a').attributes('href')).toBe('/sealed/mtg/product-1')

    await wrapper.get('a').trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.fullPath).toBe('/sealed/mtg/product-1')
  })

  it('keeps card movers on the collection page and opens the card query modal', async () => {
    const card = makeCard()
    const { router, wrapper } = await mountRow({
      card,
      quantity: 1,
      foil_quantity: 0,
      value_now: '8.00',
      value_prev: '10.00',
      change_usd: '-2.00',
      change_pct: -20,
    })

    expect(wrapper.find('.card-image-stub').exists()).toBe(true)
    expect(wrapper.text()).toContain('TST · #42')
    expect(wrapper.find('[aria-label="1 owned"]').exists()).toBe(true)
    expect(wrapper.get('a').attributes('href')).toBe('/cards/mtg/cards/card-1')

    await wrapper.get('a').trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/collection/mtg')
    expect(router.currentRoute.value.query.card).toBe('card-1')
  })

  it('shows a foil count alongside the total when some copies are foil', async () => {
    const card = makeCard()
    const { wrapper } = await mountRow({
      card,
      quantity: 2,
      foil_quantity: 1,
      value_now: '30.00',
      value_prev: '20.00',
      change_usd: '10.00',
      change_pct: 50,
    })

    // Total is regular + foil (3); the separate foil chip counts just the foils (1).
    expect(wrapper.find('[aria-label="3 owned"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="1 foil"]').exists()).toBe(true)
  })
})
