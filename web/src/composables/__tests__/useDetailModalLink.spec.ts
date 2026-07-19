import { describe, it, expect, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { useDetailModalLink } from '../useDetailModalLink'

// A host that surfaces the composable so a test can drive its imperative surface directly, the
// way a tile's click handler or a "What's in the box" row does — the shared seam CardTile,
// ProductTile, ProductContents, and ProductContainers all go through (issue #485).
let link: ReturnType<typeof useDetailModalLink>
const Host = defineComponent({
  setup() {
    link = useDetailModalLink()
    return () => h('div')
  },
})

const routes = [
  { path: '/sealed/:game', component: Host },
  { path: '/sealed/:game/:id', component: Host },
  { path: '/cards/:game', component: Host },
  { path: '/cards/:game/cards/:id', component: Host },
  // A route with no `:game` path param (the public deck page): the game must ride the query.
  { path: '/u/:handle/decks/:id', component: Host },
]

async function at(path: string): Promise<Router> {
  const router = createRouter({ history: createMemoryHistory(), routes })
  await router.push(path)
  await router.isReady()
  mount({ template: '<router-view />' }, { global: { plugins: [router] } })
  return router
}

describe('useDetailModalLink', () => {
  it('resolves each surface to its canonical full page for the anchor href', async () => {
    await at('/sealed/mtg')
    expect(link.hrefFor('card', 'mtg', 'c1')).toBe('/cards/mtg/cards/c1')
    expect(link.hrefFor('product', 'mtg', 'p1')).toBe('/sealed/mtg/p1')
  })

  it('opens a card modal in place, preserving the underlying list state', async () => {
    const router = await at('/sealed/mtg?q=box&sort=name')
    link.open('card', 'mtg', 'c1')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/sealed/mtg')
    expect(router.currentRoute.value.query).toEqual({ q: 'box', sort: 'name', card: 'c1' })
  })

  it('opens a product modal in place via router.push, so Back closes the modal', async () => {
    const router = await at('/sealed/mtg')
    // Spy after at()'s own setup push so only the open() call is counted.
    const push = vi.spyOn(router, 'push')
    const replace = vi.spyOn(router, 'replace')
    link.open('product', 'mtg', 'p1')
    await flushPromises()
    expect(router.currentRoute.value.query).toEqual({ product: 'p1' })
    // A tile/link click is a forward history entry the browser's Back can undo — never `replace`,
    // which would strand the user with no way to close the modal (DetailDialogShell reserves
    // `replace` for arrow-stepping). Pin it here at the one shared seam the four surfaces share.
    expect(push).toHaveBeenCalledTimes(1)
    expect(replace).not.toHaveBeenCalled()
  })

  it('swaps an open card modal for a product, remembering the card as the origin', async () => {
    const router = await at('/sealed/mtg?card=c1')
    link.open('product', 'mtg', 'p1')
    await flushPromises()
    expect(router.currentRoute.value.query.card).toBeUndefined()
    expect(router.currentRoute.value.query.product).toBe('p1')
    expect(router.currentRoute.value.query.openedFrom).toBe('card:c1')
  })

  it('swaps an open product modal for a card, remembering the product as the origin', async () => {
    const router = await at('/sealed/mtg?product=p1')
    link.open('card', 'mtg', 'c1')
    await flushPromises()
    expect(router.currentRoute.value.query.product).toBeUndefined()
    expect(router.currentRoute.value.query.card).toBe('c1')
    expect(router.currentRoute.value.query.openedFrom).toBe('product:p1')
  })

  it("drops a leftover namespaced product-card search — it was another product's (#448)", async () => {
    // pq/psort belong to the now-closed product modal for 'old'; opening a different product clears
    // them so its list starts fresh. 'old' also becomes the back-crumb origin (issue #485).
    const router = await at('/sealed/mtg?product=old&pq=t:goblin&psort=name:desc')
    link.open('product', 'mtg', 'p1')
    await flushPromises()
    expect(router.currentRoute.value.query).toEqual({ product: 'p1', openedFrom: 'product:old' })
  })

  it('remembers the previous product as the origin on a product->product hop', async () => {
    // Opening a nested/parent product from inside a product modal (a "What's in the box" /
    // "Included in" row) remembers the product you were on so the modal can offer
    // "← Back to <it>" (issue #485); any stale cross-surface marker is replaced.
    const router = await at('/sealed/mtg?product=old&openedFrom=card:c9')
    link.open('product', 'mtg', 'p1')
    await flushPromises()
    expect(router.currentRoute.value.query).toEqual({ product: 'p1', openedFrom: 'product:old' })
  })

  it('remembers the previous card as the origin on a card->card hop', async () => {
    // The mirror of the above for cards — clicking another printing in "Other printings".
    const router = await at('/cards/mtg?card=old')
    link.open('card', 'mtg', 'c1')
    await flushPromises()
    expect(router.currentRoute.value.query).toEqual({ card: 'c1', openedFrom: 'card:old' })
  })

  it('clears the marker when re-opening the item already on screen', async () => {
    // A no-op re-open of the current item must not stash a self-referential origin.
    const router = await at('/sealed/mtg?product=p1&openedFrom=card:c9')
    link.open('product', 'mtg', 'p1')
    await flushPromises()
    expect(router.currentRoute.value.query).toEqual({ product: 'p1' })
  })

  it('carries the game in the query on a route without a :game path param', async () => {
    const router = await at('/u/alice/decks/5')
    link.open('card', 'mtg', 'c1')
    await flushPromises()
    expect(router.currentRoute.value.query).toEqual({ card: 'c1', game: 'mtg' })
  })

  it('leaves the query game alone on a route that has one in the path', async () => {
    const router = await at('/sealed/mtg/parent')
    link.open('product', 'mtg', 'p1')
    await flushPromises()
    expect(router.currentRoute.value.query).toEqual({ product: 'p1' })
  })

  describe('onActivate', () => {
    it('opens the modal on a plain left-click', async () => {
      const router = await at('/sealed/mtg')
      link.onActivate(new MouseEvent('click'), 'product', 'mtg', 'p1')
      await flushPromises()
      expect(router.currentRoute.value.query).toEqual({ product: 'p1' })
    })

    it('leaves modifier and non-primary clicks to the browser', async () => {
      const router = await at('/sealed/mtg')
      const push = vi.spyOn(router, 'push')
      link.onActivate(new MouseEvent('click', { metaKey: true }), 'product', 'mtg', 'p1')
      link.onActivate(new MouseEvent('click', { ctrlKey: true }), 'product', 'mtg', 'p1')
      link.onActivate(new MouseEvent('click', { button: 1 }), 'product', 'mtg', 'p1')
      await flushPromises()
      expect(push).not.toHaveBeenCalled()
      expect(router.currentRoute.value.query).toEqual({})
    })

    it('ignores an already-handled click', async () => {
      const router = await at('/sealed/mtg')
      const push = vi.spyOn(router, 'push')
      const event = new MouseEvent('click', { cancelable: true })
      event.preventDefault()
      link.onActivate(event, 'product', 'mtg', 'p1')
      await flushPromises()
      expect(push).not.toHaveBeenCalled()
    })
  })
})
