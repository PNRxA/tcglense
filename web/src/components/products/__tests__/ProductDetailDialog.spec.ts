import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import ProductDetailDialog from '../ProductDetailDialog.vue'
import { useProductNavStore } from '@/stores/productNav'

let wrapper: VueWrapper

// Open over the sealed browse route, whose own `?sort=name` stands in for the list state the
// modal must leave alone. `extraQuery` appends further keys (a browse search, the modal's own
// namespaced card-search keys).
async function open(product: string, ids: string[] = ['a', 'b', 'c'], extraQuery = '') {
  const router: Router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/sealed/:game', component: { template: '<div />' } },
      { path: '/sealed/:game/:id', component: { template: '<div />' } },
    ],
  })
  const pinia = createPinia()
  setActivePinia(pinia)
  useProductNavStore().register({ game: 'mtg', ids })
  await router.push(`/sealed/mtg?sort=name&product=${product}${extraQuery}`)
  await router.isReady()

  wrapper = mount(ProductDetailDialog, {
    attachTo: document.body,
    global: {
      plugins: [router, pinia],
      stubs: { ProductDetailContent: true },
    },
  })
  await flushPromises()
  return router
}

function byLabel(label: string): HTMLButtonElement | null {
  return document.body.querySelector(`[aria-label="${label}"]`)
}

// The "← Back to <origin>" crumb has no aria-label — its accessible name is its text. With no
// query layer mounted here it falls back to the generic noun ("Back to card"), which is enough to
// locate and click it. Returns null when no crumb is rendered.
function crumbButton(): HTMLButtonElement | null {
  return (
    Array.from(document.body.querySelectorAll('button')).find((b) =>
      (b.textContent ?? '').includes('Back to'),
    ) ?? null
  )
}

function dialogEl(): HTMLElement {
  const el = document.body.querySelector('[role="dialog"]')
  if (!el) throw new Error('dialog is not open')
  return el as HTMLElement
}

// The key handler lives on the dialog's own content (not window), so a keydown must originate
// inside it to be seen — mirroring how a real keypress only reaches it while the modal is focused.
function pressArrow(key: 'ArrowLeft' | 'ArrowRight', init: KeyboardEventInit = {}) {
  dialogEl().dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, ...init }))
}

describe('ProductDetailDialog', () => {
  afterEach(() => {
    wrapper?.unmount()
    document.body.innerHTML = ''
  })

  it('opens from ?product and links to the canonical full page', async () => {
    await open('b')
    expect(dialogEl()).not.toBeNull()
    expect(document.body.querySelector('a[href="/sealed/mtg/b"]')?.textContent).toContain(
      'Open full page',
    )
  })

  it('hands the body the game + id resolved from the URL', async () => {
    // The shell owns that resolution and passes it down its slot, so a body mounted on the
    // wrong product (or on nothing at all) would be invisible to the header assertions above.
    await open('b')
    const body = dialogEl().querySelector('product-detail-content-stub')
    expect(body?.getAttribute('game')).toBe('mtg')
    expect(body?.getAttribute('id')).toBe('b')
  })

  it('steps through the underlying product grid with replace', async () => {
    const router = await open('b')
    const replace = vi.spyOn(router, 'replace')
    byLabel('Next sealed product')!.click()
    await flushPromises()

    expect(replace).toHaveBeenCalledTimes(1)
    expect(router.currentRoute.value.query.product).toBe('c')
    expect(dialogEl().textContent).toContain('3 / 3')
  })

  it('steps with arrow keys without hijacking quantity inputs', async () => {
    const router = await open('b')
    pressArrow('ArrowLeft')
    await flushPromises()
    expect(router.currentRoute.value.query.product).toBe('a')

    const input = document.createElement('input')
    dialogEl().appendChild(input)
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }))
    await flushPromises()
    expect(router.currentRoute.value.query.product).toBe('a')
  })

  it('ignores an arrow with a modifier held (leaves browser shortcuts alone)', async () => {
    // Cmd/Ctrl+Arrow is the browser's own Back/Forward — stepping the product as well would
    // fight the navigation the user actually asked for.
    const router = await open('b')
    pressArrow('ArrowRight', { metaKey: true })
    await flushPromises()
    expect(router.currentRoute.value.query.product).toBe('b')

    pressArrow('ArrowRight', { ctrlKey: true })
    await flushPromises()
    expect(router.currentRoute.value.query.product).toBe('b')
  })

  it('closes by removing only modal state', async () => {
    const router = await open('b')
    byLabel('Close')!.click()
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ sort: 'name' })
  })

  it('takes its namespaced card search with it, leaving the browse’s search/sort', async () => {
    const router = await open('b', ['a', 'b', 'c'], '&q=bloomburrow&pq=t:goblin&psort=name:desc')
    byLabel('Close')!.click()
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ sort: 'name', q: 'bloomburrow' })
  })

  it('resets its namespaced card search when stepping to the next product (#448)', async () => {
    // A search typed for product b is b's state, not the overlay's session: stepping to c must
    // not carry it (the full page drops `?q=`/`?sort=` the same way, via a fresh link URL).
    const router = await open('b', ['a', 'b', 'c'], '&q=bloomburrow&pq=t:goblin&psort=name:desc')
    byLabel('Next sealed product')!.click()
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({
      sort: 'name',
      q: 'bloomburrow',
      product: 'c',
    })
  })

  it('keeps a deep-linked ?pq= on open (a shared filtered modal stays filtered)', async () => {
    // Only transitions AWAY from a product (step / swap / close) drop the namespaced keys;
    // arriving with `?product=` + `?pq=` is the shareable-filtered-modal deep link (#443),
    // which must open still filtered — nothing may strip the keys at mount.
    const router = await open('b', ['a', 'b', 'c'], '&pq=t:goblin')
    expect(dialogEl()).not.toBeNull()
    expect(router.currentRoute.value.query.pq).toBe('t:goblin')
  })

  it('hides navigation for a deep-linked product outside the registered grid', async () => {
    await open('z')
    expect(byLabel('Previous sealed product')).toBeNull()
    expect(byLabel('Next sealed product')).toBeNull()
  })
})

describe('ProductDetailDialog origin crumb', () => {
  afterEach(() => {
    wrapper?.unmount()
    document.body.innerHTML = ''
  })

  it('offers a back crumb when opened from a card, returning to that card', async () => {
    const router = await open('b', ['a', 'b', 'c'], '&openedFrom=card:card-7')
    expect(crumbButton()).not.toBeNull()

    crumbButton()!.click()
    await flushPromises()

    // The product closes and the remembered card reopens; list state (sort) stays untouched.
    expect(router.currentRoute.value.query).toEqual({ sort: 'name', card: 'card-7' })
  })

  it('drops its namespaced card search and the marker on the return trip', async () => {
    const router = await open(
      'b',
      ['a', 'b', 'c'],
      '&openedFrom=card:card-7&pq=t:goblin&psort=name:desc',
    )
    crumbButton()!.click()
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ sort: 'name', card: 'card-7' })
  })

  it('shows no crumb without a from marker', async () => {
    await open('b')
    expect(crumbButton()).toBeNull()
  })

  it('offers a back crumb for a same-surface origin, returning to that product', async () => {
    // A product opened from another product's "What's in the box" / "Included in" remembers it, so
    // the crumb points back to that product (issue #485) — the same one-tap return a card swap gives.
    const router = await open('b', ['a', 'b', 'c'], '&openedFrom=product:other')
    expect(crumbButton()).not.toBeNull()

    crumbButton()!.click()
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ sort: 'name', product: 'other' })
  })

  it('ignores a self-referential marker (never points back at the open product)', async () => {
    // A stray marker whose id IS the open product would point the crumb at itself — suppress it.
    await open('b', ['a', 'b', 'c'], '&openedFrom=product:b')
    expect(crumbButton()).toBeNull()
  })

  it('drops the marker on close', async () => {
    const router = await open('b', ['a', 'b', 'c'], '&openedFrom=card:card-7')
    byLabel('Close')!.click()
    await flushPromises()
    expect(router.currentRoute.value.query).toEqual({ sort: 'name' })
  })

  it('drops the marker when stepping to a neighbour', async () => {
    const router = await open('b', ['a', 'b', 'c'], '&openedFrom=card:card-7')
    byLabel('Next sealed product')!.click()
    await flushPromises()
    expect(router.currentRoute.value.query).toEqual({ sort: 'name', product: 'c' })
  })

  it('returns with push, not replace, so Back can undo the trip', async () => {
    // Mirrors the prev/next replace assertion: the return is a forward step Back should undo, not
    // an in-place rewrite.
    const router = await open('b', ['a', 'b', 'c'], '&openedFrom=card:card-7')
    const pushSpy = vi.spyOn(router, 'push')
    const replaceSpy = vi.spyOn(router, 'replace')
    crumbButton()!.click()
    await flushPromises()
    expect(pushSpy).toHaveBeenCalledTimes(1)
    expect(replaceSpy).not.toHaveBeenCalled()
  })

  it('names the origin from a warm query cache (shell forwards the origin id/kind/game)', async () => {
    // With the query layer present, the crumb resolves the real name — proving the shell hands the
    // crumb the ORIGIN's kind/id/game, not the open product's (a mis-binding would fail to resolve).
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/sealed/:game', component: { template: '<div />' } },
        { path: '/sealed/:game/:id', component: { template: '<div />' } },
      ],
    })
    const pinia = createPinia()
    setActivePinia(pinia)
    useProductNavStore().register({ game: 'mtg', ids: ['a', 'b', 'c'] })
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    queryClient.setQueryData(['card', 'mtg', 'card-7'], { id: 'card-7', name: 'Lightning Bolt' })
    await router.push('/sealed/mtg?sort=name&product=b&openedFrom=card:card-7')
    await router.isReady()

    wrapper = mount(ProductDetailDialog, {
      attachTo: document.body,
      global: {
        plugins: [router, pinia, [VueQueryPlugin, { queryClient }]],
        stubs: { ProductDetailContent: true },
      },
    })
    await flushPromises()

    expect(crumbButton()?.textContent).toContain('Back to Lightning Bolt')
  })
})

describe('ProductDetailDialog game resolution', () => {
  let localWrapper: VueWrapper

  // Mount over an arbitrary route with the given query. Product tiles reach a route with no
  // `:game` path param through the public deck page (`/u/:handle/decks/:id`): a card tile there
  // opens the card modal, whose "Sealed products" section is a real ProductGrid. Those tiles
  // carry the game in the query, and the dialog must resolve it from there to open.
  async function mountAt(fullPath: string) {
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/u/:handle/decks/:id', component: { template: '<div />' } },
        { path: '/sealed/:game', component: { template: '<div />' } },
        { path: '/sealed/:game/:id', component: { template: '<div />' } },
      ],
    })
    const pinia = createPinia()
    setActivePinia(pinia)
    await router.push(fullPath)
    await router.isReady()
    localWrapper = mount(ProductDetailDialog, {
      attachTo: document.body,
      global: { plugins: [router, pinia], stubs: { ProductDetailContent: true } },
    })
    await flushPromises()
    return router
  }

  afterEach(() => {
    localWrapper?.unmount()
    document.body.innerHTML = ''
  })

  it('opens on a game-less route when the game is carried in the query', async () => {
    await mountAt('/u/alice/decks/5?product=x&game=mtg')
    expect(document.body.querySelector('[role="dialog"]')).not.toBeNull()
    expect(document.body.querySelector('a[href="/sealed/mtg/x"]')).not.toBeNull()
  })

  it('stays closed on a game-less route with no game anywhere', async () => {
    await mountAt('/u/alice/decks/5?product=x')
    expect(document.body.querySelector('[role="dialog"]')).toBeNull()
  })

  it('prefers the path param over the query when both could apply', async () => {
    // A browse route carries :game in the path; the query fallback must not shadow it.
    await mountAt('/sealed/mtg?product=x')
    expect(document.body.querySelector('[role="dialog"]')).not.toBeNull()
    expect(document.body.querySelector('a[href="/sealed/mtg/x"]')).not.toBeNull()
  })

  it('takes the carried game with it on close', async () => {
    // `?game=` exists only to feed this modal on a route that can't, so closing must leave no
    // trace of it on the deck page underneath.
    const router = await mountAt('/u/alice/decks/5?product=x&game=mtg')
    byLabel('Close')!.click()
    await flushPromises()
    expect(router.currentRoute.value.query).toEqual({})
    expect(router.currentRoute.value.path).toBe('/u/alice/decks/5')
  })

  it('preserves the carried game on the crumb return trip (unlike close, which drops it)', async () => {
    // A product opened from a card on the deck page returns to that card; `?game=` must survive so
    // the reopened card modal can still resolve its game (close drops it — goToOrigin must not).
    const router = await mountAt('/u/alice/decks/5?product=y&game=mtg&openedFrom=card:x')
    crumbButton()!.click()
    await flushPromises()
    expect(router.currentRoute.value.query).toEqual({ card: 'x', game: 'mtg' })
    expect(router.currentRoute.value.path).toBe('/u/alice/decks/5')
  })
})
