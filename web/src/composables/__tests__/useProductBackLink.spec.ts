import { describe, it, expect } from 'vitest'
import { computed, defineComponent, h, nextTick } from 'vue'
import { mount } from '@vue/test-utils'
import { createRouter, createWebHistory, useRoute } from 'vue-router'
import { useProductBackLink } from '../useProductBackLink'

// The real named routes a sealed product can be opened from: the per-game sealed browse
// (a product tile), the wish list's sealed section, and a card's "Sealed products"
// section — the card's full page, or the browse-grid card modal (`?card=<id>`) that can
// sit over any list route. Each renders a placeholder except the product route, which
// hosts the composable.
const routes = [
  { path: '/', name: 'home', component: { template: '<div />' } },
  { path: '/cards/:game', name: 'game', component: { template: '<div />' } },
  { path: '/cards/:game/cards', name: 'game-cards', component: { template: '<div />' } },
  { path: '/cards/:game/sets/:code', name: 'set', component: { template: '<div />' } },
  { path: '/cards/:game/cards/:id', name: 'card', component: { template: '<div />' } },
  { path: '/sealed', name: 'sealed', component: { template: '<div />' } },
  { path: '/sealed/:game', name: 'game-sealed', component: { template: '<div />' } },
  { path: '/sealed/:game/:id', name: 'sealed-product', component: Host() },
  { path: '/wishlist/:game', name: 'game-wishlist', component: { template: '<div />' } },
  { path: '/collection/:game', name: 'game-collection', component: { template: '<div />' } },
]

// Product-route component: calls the composable with the route's `:game` and renders the
// resolved link so the test can read its `to`/label.
function Host() {
  return defineComponent({
    setup() {
      const route = useRoute()
      const game = computed(() => String(route.params.game))
      const link = useProductBackLink(game)
      return () => h('a', { href: link.value.to }, link.value.label)
    },
  })
}

// Navigate through `from` (the referrer, if any) then to the product page, mount the
// tree, and return the rendered back link. Mounting *after* the pushes means the product
// page's `history.state.back` is already the referrer, exactly as on a real click. A
// neutral `/` base is pushed first so the product page's `state.back` depends only on the
// last two entries — deterministic regardless of test order over the shared jsdom
// history (memory history doesn't track `state.back`, so this uses web history).
async function backLink(from?: string) {
  const router = createRouter({ history: createWebHistory(), routes })
  await router.push('/')
  if (from) await router.push(from)
  await router.push('/sealed/mtg/dummy-product')
  await router.isReady()
  const wrapper = mount({ template: '<router-view />' }, { global: { plugins: [router] } })
  await nextTick()
  const a = wrapper.find('a')
  return { to: a.attributes('href'), label: a.text() }
}

describe('useProductBackLink', () => {
  it('falls back to the sealed browse when the referrer is unrelated', async () => {
    expect(await backLink()).toEqual({ to: '/sealed/mtg', label: 'Sealed products' })
  })

  it('returns to the sealed browse it was opened from', async () => {
    expect(await backLink('/sealed/mtg')).toEqual({ to: '/sealed/mtg', label: 'Sealed products' })
  })

  it("preserves the sealed browse's search/filter/page state", async () => {
    expect(await backLink('/sealed/mtg?set=blb')).toEqual({
      to: '/sealed/mtg?set=blb',
      label: 'Sealed products',
    })
  })

  it('returns to the wish list it was opened from, preserving its sealed-product page', async () => {
    expect(await backLink('/wishlist/mtg?page=2')).toEqual({
      to: '/wishlist/mtg?page=2',
      label: 'Wish list',
    })
  })

  it('returns to the collection it was opened from', async () => {
    expect(await backLink('/collection/mtg')).toEqual({
      to: '/collection/mtg',
      label: 'Collection',
    })
  })

  it('returns to another sealed product it was opened from (a linked sub-product)', async () => {
    expect(await backLink('/sealed/mtg/parent-product')).toEqual({
      to: '/sealed/mtg/parent-product',
      label: 'Sealed product',
    })
  })

  it("returns to the card's full detail page it was opened from", async () => {
    expect(await backLink('/cards/mtg/cards/dummy-card')).toEqual({
      to: '/cards/mtg/cards/dummy-card',
      label: 'Card',
    })
  })

  it('re-opens the card modal (keeping `?card`) it was opened from over a set list', async () => {
    expect(await backLink('/cards/mtg/sets/blb?card=dummy-card')).toEqual({
      to: '/cards/mtg/sets/blb?card=dummy-card',
      label: 'Card',
    })
  })

  it("re-opens the card modal over the all-cards list, preserving that list's search", async () => {
    expect(await backLink('/cards/mtg/cards?q=bolt&card=dummy-card')).toEqual({
      to: '/cards/mtg/cards?q=bolt&card=dummy-card',
      label: 'Card',
    })
  })
})
