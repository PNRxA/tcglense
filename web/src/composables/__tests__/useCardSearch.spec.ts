import { describe, it, expect } from 'vitest'

import { computed, defineComponent, h, nextTick, ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { ALL_CARDS_DEFAULT_SORT, ALL_CARDS_SORT_OPTIONS } from '@/lib/cardSort'
import { useCardSearch, useDropFilter } from '../useCardSearch'

const VALID_SORTS = ALL_CARDS_SORT_OPTIONS.map((option) => option.value)

function makeRouter() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/cards/:game/cards', name: 'cards', component: { template: '<div />' } },
      { path: '/cards/:game/cards/:id', name: 'card', component: { template: '<div />' } },
      { path: '/cards/:game/sets/:code', name: 'set', component: { template: '<div />' } },
    ],
  })
}

// Mount a throwaway component that just runs the composable, so useRoute/useRouter
// resolve and the test can drive the returned state. The component lives outside
// <RouterView>, so navigating the router doesn't unmount it — re-mounting a fresh
// harness stands in for the list view re-mounting after Back.
function mountSearch(router: Router) {
  let api!: ReturnType<typeof useCardSearch>
  const harness = mount(
    defineComponent({
      setup() {
        api = useCardSearch(ALL_CARDS_DEFAULT_SORT, VALID_SORTS)
        return () => h('div')
      },
    }),
    { global: { plugins: [router] } },
  )
  return { api, harness }
}

async function start(at: string) {
  const router = makeRouter()
  await router.push(at)
  await router.isReady()
  const { api, harness } = mountSearch(router)
  await nextTick()
  return { router, api, harness }
}

const query = (router: Router) => router.currentRoute.value.query

describe('useCardSearch', () => {
  it('hydrates page, search and sort from the URL', async () => {
    const { api } = await start('/cards/mtg/cards?page=3&q=goblin&sort=price:desc')
    expect(api.page.value).toBe(3)
    expect(api.query.value).toBe('goblin')
    expect(api.searchInput.value).toBe('goblin')
    expect(api.sort.value).toBe('price:desc')
  })

  it('writes the page into the URL without leaving page=1 behind', async () => {
    const { router, api } = await start('/cards/mtg/cards')
    api.page.value = 2
    await flushPromises()
    expect(query(router).page).toBe('2')

    api.page.value = 1
    await flushPromises()
    expect(query(router).page).toBeUndefined()
  })

  it('restarts paging when the sort changes and clears the default sort from the URL', async () => {
    const { router, api } = await start('/cards/mtg/cards?page=4')
    api.sort.value = 'name:desc'
    await flushPromises()
    expect(query(router).sort).toBe('name:desc')
    expect(query(router).page).toBeUndefined()
    expect(api.page.value).toBe(1)

    api.sort.value = ALL_CARDS_DEFAULT_SORT
    await flushPromises()
    expect(query(router).sort).toBeUndefined()
    expect(api.sort.value).toBe(ALL_CARDS_DEFAULT_SORT)
  })

  it('debounces the search box into ?q and restarts paging', async () => {
    const { router, api } = await start('/cards/mtg/cards?page=5')
    api.searchInput.value = 'dragon'
    // Below the 300ms debounce: nothing committed yet.
    await flushPromises()
    expect(query(router).q).toBeUndefined()

    await new Promise((resolve) => setTimeout(resolve, 330))
    await flushPromises()
    expect(query(router).q).toBe('dragon')
    expect(query(router).page).toBeUndefined()
  })

  it('clamps an unknown ?sort to the default instead of forwarding it to the API', async () => {
    const { api } = await start('/cards/mtg/cards?sort=not-a-real-sort')
    expect(api.sort.value).toBe(ALL_CARDS_DEFAULT_SORT)
  })

  it('re-clamps the committed sort when reactive defaults/valid sets change (mode swap)', async () => {
    // The collection view swaps its sort set with the show-ghosts toggle: passing getters
    // lets a URL sort that's valid in one mode fall back to the other mode's default when
    // the mode flips (so a stale sort is never forwarded to the API).
    const router = makeRouter()
    await router.push('/cards/mtg/cards?sort=y:asc')
    await router.isReady()

    const ghosts = ref(false)
    const validSorts = computed(() => (ghosts.value ? ['p:asc', 'q:asc'] : ['x:asc', 'y:asc']))
    const defaultSort = computed(() => (ghosts.value ? 'p:asc' : 'x:asc'))

    let api!: ReturnType<typeof useCardSearch>
    mount(
      defineComponent({
        setup() {
          api = useCardSearch(defaultSort, validSorts)
          return () => h('div')
        },
      }),
      { global: { plugins: [router] } },
    )
    await nextTick()

    // `y:asc` is valid in the owned mode → honoured.
    expect(api.sort.value).toBe('y:asc')

    // Flip to ghost mode: `y:asc` is no longer valid → clamps to ghost mode's default.
    ghosts.value = true
    await nextTick()
    expect(api.sort.value).toBe('p:asc')
  })

  it('preserves unrelated query keys (a set view scope) when paging', async () => {
    const { router, api } = await start('/cards/mtg/sets/abc?related=1&from=xyz')
    api.page.value = 2
    await flushPromises()
    expect(query(router).related).toBe('1')
    expect(query(router).from).toBe('xyz')
    expect(query(router).page).toBe('2')
  })

  it('cancels a not-yet-committed search when navigating to another list (no leak)', async () => {
    // Models SetView/CardsBrowseView being reused across a param-only navigation: the
    // composable instance (and its debounce timer) survives, so a half-typed search
    // must not commit onto the destination.
    const { router, api } = await start('/cards/mtg/sets/aaa')
    api.searchInput.value = 'go'
    await flushPromises() // still inside the 300ms debounce — nothing committed yet

    await router.replace({ path: '/cards/mtg/sets/bbb', query: {} })
    await flushPromises()
    // The box resyncs to the (empty) destination query immediately…
    expect(api.searchInput.value).toBe('')

    // …and the pending 'go' never lands on set bbb.
    await new Promise((resolve) => setTimeout(resolve, 330))
    await flushPromises()
    expect(query(router).q).toBeUndefined()
    expect(router.currentRoute.value.path).toBe('/cards/mtg/sets/bbb')
  })

  it('resyncs the search box to the destination query on navigation', async () => {
    const { router, api } = await start('/cards/mtg/sets/aaa?q=elf')
    expect(api.searchInput.value).toBe('elf')
    await router.replace({ path: '/cards/mtg/sets/bbb', query: { q: 'goblin' } })
    await flushPromises()
    expect(api.searchInput.value).toBe('goblin')
    expect(api.query.value).toBe('goblin')
  })

  it('remembers the list state across opening a card and pressing Back (issue #58)', async () => {
    const { router, api } = await start('/cards/mtg/cards')
    // Search first (which restarts paging), then page into the results — the order a
    // user actually browses in.
    api.searchInput.value = 'goblin'
    await new Promise((resolve) => setTimeout(resolve, 330))
    await flushPromises()
    api.page.value = 3
    await flushPromises()

    const listUrl = router.currentRoute.value.fullPath
    expect(listUrl).toContain('page=3')
    expect(listUrl).toContain('q=goblin')

    // Open a card, then go Back — the list URL (with its state) is restored…
    await router.push('/cards/mtg/cards/some-card')
    router.back()
    await flushPromises()
    expect(router.currentRoute.value.fullPath).toBe(listUrl)

    // …and a fresh mount of the list reads that state straight back.
    const { api: restored } = mountSearch(router)
    await nextTick()
    expect(restored.page.value).toBe(3)
    expect(restored.query.value).toBe('goblin')
    expect(restored.searchInput.value).toBe('goblin')
  })
})

// Same throwaway-harness pattern as mountSearch, for the by-drop "filter drops by name"
// composable (SetView's ?drop= box).
function mountDropFilter(router: Router) {
  let api!: ReturnType<typeof useDropFilter>
  const harness = mount(
    defineComponent({
      setup() {
        api = useDropFilter()
        return () => h('div')
      },
    }),
    { global: { plugins: [router] } },
  )
  return { api, harness }
}

async function startDrop(at: string) {
  const router = makeRouter()
  await router.push(at)
  await router.isReady()
  const { api } = mountDropFilter(router)
  await nextTick()
  return { router, api }
}

describe('useDropFilter', () => {
  it('hydrates the box and committed filter from ?drop', async () => {
    const { api } = await startDrop('/cards/mtg/sets/sld?drop=bloom')
    expect(api.dropQuery.value).toBe('bloom')
    expect(api.dropInput.value).toBe('bloom')
  })

  it('debounces the box into ?drop and restarts paging', async () => {
    const { router, api } = await startDrop('/cards/mtg/sets/sld?page=4')
    api.dropInput.value = 'galaxy'
    // Below the 300ms debounce: nothing committed yet.
    await flushPromises()
    expect(query(router).drop).toBeUndefined()

    await new Promise((resolve) => setTimeout(resolve, 330))
    await flushPromises()
    expect(query(router).drop).toBe('galaxy')
    // A new filter restarts paging — the old page is meaningless once the list narrows.
    expect(query(router).page).toBeUndefined()
  })

  it('drops the ?drop key when the box is cleared', async () => {
    const { router, api } = await startDrop('/cards/mtg/sets/sld?drop=bloom')
    api.dropInput.value = ''
    await new Promise((resolve) => setTimeout(resolve, 330))
    await flushPromises()
    expect(query(router).drop).toBeUndefined()
    expect(api.dropQuery.value).toBe('')
  })

  it('preserves the card search ?q when committing a drop filter', async () => {
    const { router, api } = await startDrop('/cards/mtg/sets/sld?q=elf')
    api.dropInput.value = 'bloom'
    await new Promise((resolve) => setTimeout(resolve, 330))
    await flushPromises()
    expect(query(router).drop).toBe('bloom')
    expect(query(router).q).toBe('elf')
  })

  it('resyncs the box to the destination query on navigation (no leak)', async () => {
    const { router, api } = await startDrop('/cards/mtg/sets/sld?drop=bloom')
    api.dropInput.value = 'gal' // half-typed, inside the debounce
    await flushPromises()

    await router.replace({ path: '/cards/mtg/sets/other', query: {} })
    await flushPromises()
    // The box resyncs to the (empty) destination immediately…
    expect(api.dropInput.value).toBe('')

    // …and the pending 'gal' never lands on the destination set.
    await new Promise((resolve) => setTimeout(resolve, 330))
    await flushPromises()
    expect(query(router).drop).toBeUndefined()
  })

  it('drops a pending edit when the view leaves by-drop (no phantom ?drop on flat view)', async () => {
    // Toggling to All-cards is a same-path ?view=all change the path watcher can't see, so
    // useDropFilter takes the by-drop flag to cancel a mid-debounce keystroke itself.
    const router = makeRouter()
    await router.push('/cards/mtg/sets/sld')
    await router.isReady()
    const active = ref(true)
    let api!: ReturnType<typeof useDropFilter>
    mount(
      defineComponent({
        setup() {
          api = useDropFilter(active)
          return () => h('div')
        },
      }),
      { global: { plugins: [router] } },
    )
    await nextTick()

    api.dropInput.value = 'galaxy' // half-typed, inside the 300ms debounce
    await flushPromises()

    // Leave the by-drop view before the debounce fires (same path, ?view=all).
    active.value = false
    await router.replace({ query: { view: 'all' } })
    await nextTick()
    // The box resyncs to the committed (empty) value…
    expect(api.dropInput.value).toBe('')

    // …and the pending 'galaxy' never lands a phantom ?drop on the flat-view URL.
    await new Promise((resolve) => setTimeout(resolve, 330))
    await flushPromises()
    expect(query(router).drop).toBeUndefined()
    expect(query(router).view).toBe('all')
  })
})
