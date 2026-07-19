import { describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Product } from '@/lib/api'
import ProductTile from '../ProductTile.vue'

const product: Product = {
  id: 'product-1',
  name: 'Test Bundle',
  set_code: 'tst',
  set_name: 'Test Set',
  product_type: 'bundle',
  url: null,
  has_image: false,
  prices: { usd: '42.00', usd_foil: null },
  msrp: null,
  released_at: null,
}

async function mountTile(path = '/sealed/mtg?sort=name') {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/u/:handle/decks/:id', component: { template: '<div />' } },
      { path: '/sealed/:game', component: { template: '<div />' } },
      { path: '/sealed/:game/:id', component: { template: '<div />' } },
    ],
  })
  await router.push(path)
  await router.isReady()
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = mount(ProductTile, {
    props: { game: 'mtg', product },
    global: {
      plugins: [createPinia(), router, [VueQueryPlugin, { queryClient }]],
      stubs: { ProductImage: true },
    },
  })
  return { wrapper, router }
}

describe('ProductTile detail modal', () => {
  it('keeps the canonical product page in the anchor href', async () => {
    const { wrapper } = await mountTile()
    expect(wrapper.get('a').attributes('href')).toBe('/sealed/mtg/product-1')
  })

  it('opens a plain left-click in-place via ?product while preserving list state', async () => {
    const { wrapper, router } = await mountTile()
    await wrapper.get('a').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.path).toBe('/sealed/mtg')
    expect(router.currentRoute.value.query).toEqual({ sort: 'name', product: 'product-1' })
  })

  it('leaves modifier-click navigation to the browser', async () => {
    const { wrapper, router } = await mountTile()
    const push = vi.spyOn(router, 'push')
    // Suppress jsdom's unimplemented real-document navigation after the component has had
    // a chance to leave the modifier click untouched.
    wrapper.get('a').element.addEventListener('click', (event) => event.preventDefault(), {
      once: true,
    })
    await wrapper.get('a').trigger('click', { ctrlKey: true })
    expect(push).not.toHaveBeenCalled()
    expect(router.currentRoute.value.query).toEqual({ sort: 'name' })
  })

  it('swaps an open card modal for the product modal, remembering the card', async () => {
    const { wrapper, router } = await mountTile('/sealed/mtg?card=card-1')
    await wrapper.get('a').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.query.card).toBeUndefined()
    expect(router.currentRoute.value.query.product).toBe('product-1')
    // The card the product was opened from is stashed so the modal can offer "← Back to <card>".
    expect(router.currentRoute.value.query.openedFrom).toBe('card:card-1')
  })

  it('drops a leftover namespaced card search — it was another product’s (#448)', async () => {
    // `?pq=`/`?psort=` only ever belong to an open product modal; anything still in the URL when
    // a tile opens this product was typed for a different one, so the list must start fresh.
    const { wrapper, router } = await mountTile(
      '/sealed/mtg?card=card-1&pq=t:goblin&psort=name:desc',
    )
    await wrapper.get('a').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({
      product: 'product-1',
      openedFrom: 'card:card-1',
    })
  })

  it('remembers the product it was opened from on a product->product hop', async () => {
    // Reaching one product from another (a linked box/pack in "What's in the box" / "Included in")
    // now remembers that product so the modal can offer "← Back to <product>" (issue #485), the
    // same one-tap return a card<->product swap gives. Any stale cross-surface marker is replaced.
    const { wrapper, router } = await mountTile('/sealed/mtg?product=old&openedFrom=card:card-9')
    await wrapper.get('a').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({
      product: 'product-1',
      openedFrom: 'product:old',
    })
  })

  it('carries the game in the query on a route without a :game path param', async () => {
    // A product grid can render where the path has no `:game` to feed the shared dialog: the
    // public deck page reaches one through the card modal's "Sealed products" section. The tile
    // hands its own game over in the query instead, so the dialog can still resolve it —
    // CardTile's idiom, and the only thing that makes the modal openable there.
    const { wrapper, router } = await mountTile('/u/alice/decks/5')
    await wrapper.get('a').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ product: 'product-1', game: 'mtg' })
  })

  it('leaves the query game alone on a route that has one in the path', async () => {
    const { wrapper, router } = await mountTile('/sealed/mtg')
    await wrapper.get('a').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ product: 'product-1' })
  })
})
