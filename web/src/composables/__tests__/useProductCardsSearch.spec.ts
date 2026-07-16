import { describe, it, expect } from 'vitest'

import { defineComponent, h, nextTick, type Ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { PRODUCT_CARDS_DEFAULT_SORT, PRODUCT_CARDS_SORT_OPTIONS } from '@/lib/cardSort'
import {
  PRODUCT_CARDS_MODAL_SEARCH_KEYS,
  useProductCardsSearch,
  type ProductCardsSearchKeys,
} from '../useProductCardsSearch'

const VALID_SORTS = PRODUCT_CARDS_SORT_OPTIONS.map((option) => option.value)

function makeRouter() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/sealed/:game', name: 'browse', component: { template: '<div />' } },
      { path: '/sealed/:game/:id', name: 'product', component: { template: '<div />' } },
      { path: '/cards/:game/:id', name: 'card', component: { template: '<div />' } },
    ],
  })
}

// Mount a throwaway component that just runs the composable, so useRoute/useRouter resolve and
// the test can drive the returned state. It lives outside <RouterView>, so navigating the router
// doesn't unmount it — a re-mount stands in for the product view re-mounting after Back.
// `keys` defaults through the composable to the full page's plain `?q=`/`?sort=`.
function mountSearch(router: Router, keys?: ProductCardsSearchKeys) {
  let api!: ReturnType<typeof useProductCardsSearch>
  const harness = mount(
    defineComponent({
      setup() {
        api = useProductCardsSearch(PRODUCT_CARDS_DEFAULT_SORT, VALID_SORTS, keys)
        return () => h('div')
      },
    }),
    { global: { plugins: [router] } },
  )
  return { api, harness }
}

async function start(at: string, keys?: ProductCardsSearchKeys) {
  const router = makeRouter()
  await router.push(at)
  await router.isReady()
  const { api, harness } = mountSearch(router, keys)
  await nextTick()
  return { router, api, harness }
}

const query = (router: Router) => router.currentRoute.value.query

describe('useProductCardsSearch', () => {
  it('hydrates the search and sort from the URL', async () => {
    const { api } = await start('/sealed/mtg/100?q=goblin&sort=price:desc')
    expect(api.query.value).toBe('goblin')
    expect(api.searchInput.value).toBe('goblin')
    expect(api.sort.value).toBe('price:desc')
  })

  it('defaults the sort to the natural order when ?sort is absent', async () => {
    const { api } = await start('/sealed/mtg/100')
    expect(api.sort.value).toBe(PRODUCT_CARDS_DEFAULT_SORT)
  })

  it('commits a chosen sort into ?sort and drops the key when it returns to the default', async () => {
    const { router, api } = await start('/sealed/mtg/100')
    api.sort.value = 'name:desc'
    await flushPromises()
    expect(query(router).sort).toBe('name:desc')

    // The default sort rides the URL implicitly — the key is dropped for a clean canonical URL.
    api.sort.value = PRODUCT_CARDS_DEFAULT_SORT
    await flushPromises()
    expect(query(router).sort).toBeUndefined()
    expect(api.sort.value).toBe(PRODUCT_CARDS_DEFAULT_SORT)
  })

  it('clamps an unknown ?sort back to the default instead of forwarding it to the API', async () => {
    const { api } = await start('/sealed/mtg/100?sort=not-a-real-sort')
    expect(api.sort.value).toBe(PRODUCT_CARDS_DEFAULT_SORT)
  })

  it('debounces the search box into ?q (no shared page to reset)', async () => {
    const { router, api } = await start('/sealed/mtg/100')
    api.searchInput.value = 'dragon'
    // Below the 300ms debounce: nothing committed yet.
    await flushPromises()
    expect(query(router).q).toBeUndefined()

    await new Promise((resolve) => setTimeout(resolve, 330))
    await flushPromises()
    expect(query(router).q).toBe('dragon')
  })

  it('preserves unrelated query keys when committing the sort', async () => {
    const { router, api } = await start('/sealed/mtg/100?from=xyz')
    api.sort.value = 'rarity:desc'
    await flushPromises()
    expect(query(router).sort).toBe('rarity:desc')
    expect(query(router).from).toBe('xyz')
  })

  it('cancels a not-yet-committed search when navigating to another product (no leak)', async () => {
    const { router, api } = await start('/sealed/mtg/100')
    api.searchInput.value = 'go'
    await flushPromises() // still inside the 300ms debounce — nothing committed yet

    await router.replace({ path: '/sealed/mtg/200', query: {} })
    await flushPromises()
    // The box resyncs to the (empty) destination query immediately…
    expect(api.searchInput.value).toBe('')

    // …and the pending 'go' never lands on product 200.
    await new Promise((resolve) => setTimeout(resolve, 330))
    await flushPromises()
    expect(query(router).q).toBeUndefined()
    expect(router.currentRoute.value.path).toBe('/sealed/mtg/200')
  })

  it('resyncs the search box to the destination query on navigation', async () => {
    const { router, api } = await start('/sealed/mtg/100?q=elf')
    expect(api.searchInput.value).toBe('elf')
    await router.replace({ path: '/sealed/mtg/200', query: { q: 'goblin' } })
    await flushPromises()
    expect(api.searchInput.value).toBe('goblin')
    expect(api.query.value).toBe('goblin')
  })

  it('remembers the search + sort across opening a card and pressing Back (issue #58)', async () => {
    const { router, api } = await start('/sealed/mtg/100')
    api.sort.value = 'price:desc'
    api.searchInput.value = 'goblin'
    await new Promise((resolve) => setTimeout(resolve, 330))
    await flushPromises()

    const listUrl = router.currentRoute.value.fullPath
    expect(listUrl).toContain('sort=price')
    expect(listUrl).toContain('q=goblin')

    // Open a card, then go Back — the product URL (with its state) is restored…
    await router.push('/cards/mtg/some-card')
    router.back()
    await flushPromises()
    expect(router.currentRoute.value.fullPath).toBe(listUrl)

    // …and a fresh mount reads that state straight back.
    const { api: restored } = mountSearch(router)
    await nextTick()
    expect(restored.query.value).toBe('goblin')
    expect(restored.searchInput.value).toBe('goblin')
    expect((restored.sort as Ref<string>).value).toBe('price:desc')
  })
})

// The detail modal renders this list over the sealed *browse* route, whose own `useCardSearch`
// already owns `?q=`/`?sort=`. Both are URL-backed and blind to each other, so the modal takes
// namespaced keys — these are the crossings that would otherwise happen. The browse state below
// (`?q=bloomburrow&sort=price:desc`) is exactly what a user filtering the grid would have.
describe('useProductCardsSearch namespaced onto a route that owns ?q=/?sort=', () => {
  const MODAL = '/sealed/mtg?q=bloomburrow&sort=price:desc&product=100'

  it('ignores the browse’s ?q=, reading only its own ?pq=', async () => {
    const { api } = await start(MODAL, PRODUCT_CARDS_MODAL_SEARCH_KEYS)
    expect(api.query.value).toBe('')
    expect(api.searchInput.value).toBe('')

    const { api: filtered } = await start(`${MODAL}&pq=t:goblin`, PRODUCT_CARDS_MODAL_SEARCH_KEYS)
    expect(filtered.query.value).toBe('t:goblin')
    expect(filtered.searchInput.value).toBe('t:goblin')
  })

  it('ignores the browse’s ?sort=, even though the value is valid for both option sets', async () => {
    // `price:desc` is in PRODUCT_SORT_OPTIONS *and* PRODUCT_CARDS_SORT_OPTIONS, so the clamp
    // would happily pass the browse's sort through — only the key namespacing separates them.
    const { api } = await start(MODAL, PRODUCT_CARDS_MODAL_SEARCH_KEYS)
    expect(api.sort.value).toBe(PRODUCT_CARDS_DEFAULT_SORT)

    const { api: sorted } = await start(`${MODAL}&psort=name:desc`, PRODUCT_CARDS_MODAL_SEARCH_KEYS)
    expect(sorted.sort.value).toBe('name:desc')
  })

  it('commits its sort to ?psort=, leaving the browse’s ?q=/?sort= untouched', async () => {
    const { router, api } = await start(MODAL, PRODUCT_CARDS_MODAL_SEARCH_KEYS)
    api.sort.value = 'name:desc'
    await flushPromises()

    expect(query(router).psort).toBe('name:desc')
    expect(query(router).q).toBe('bloomburrow')
    expect(query(router).sort).toBe('price:desc')
    expect(query(router).product).toBe('100')
  })

  it('commits its search to ?pq=, leaving the browse’s ?q=/?sort= untouched', async () => {
    const { router, api } = await start(MODAL, PRODUCT_CARDS_MODAL_SEARCH_KEYS)
    api.searchInput.value = 't:goblin'
    await new Promise((resolve) => setTimeout(resolve, 330))
    await flushPromises()

    expect(query(router).pq).toBe('t:goblin')
    expect(query(router).q).toBe('bloomburrow')
    expect(query(router).sort).toBe('price:desc')
  })
})
