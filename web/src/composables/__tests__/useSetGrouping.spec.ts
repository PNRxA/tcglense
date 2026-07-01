import { describe, it, expect } from 'vitest'

import { defineComponent, h, nextTick, ref, type Ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import type { CardSet } from '@/lib/api'
import { useSetGrouping } from '../useSetGrouping'

// A main set (Bloomburrow) with one related sub-set (its Commander decks). findGroup
// resolves both codes to this same group, which is all the scope nav needs.
function makeSet(code: string, name: string, parent: string | null): CardSet {
  return {
    code,
    name,
    set_type: parent ? 'commander' : 'expansion',
    released_at: '2024-08-01',
    card_count: 100,
    icon_svg_uri: null,
    parent_set_code: parent,
    has_drops: false,
  }
}

const SETS: CardSet[] = [
  makeSet('blb', 'Bloomburrow', null),
  makeSet('blc', 'Bloomburrow Commander', 'blb'),
]

function makeRouter() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/cards/:game/sets/:code', name: 'set', component: { template: '<div />' } },
      // The collection reuses the composable under its own route prefix (via basePath).
      {
        path: '/collection/:game/sets/:code',
        name: 'collection-set',
        component: { template: '<div />' },
      },
    ],
  })
}

// Mount a throwaway component that just runs the composable. `code` is a ref the test
// drives to mirror the set-view prop updating after a navigation (the real view
// re-reads the route param; here we set it by hand to keep the composable instance).
function mountGrouping(router: Router, initialCode: string, basePath?: string) {
  const game = ref('mtg')
  const code: Ref<string> = ref(initialCode)
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  })
  // Seed the set list so `group` resolves synchronously (no network in tests).
  queryClient.setQueryData(['sets', 'mtg'], { data: SETS })
  let api!: ReturnType<typeof useSetGrouping>
  const harness = mount(
    defineComponent({
      setup() {
        api = useSetGrouping(game, code, basePath ? { basePath } : {})
        return () => h('div')
      },
    }),
    { global: { plugins: [router, [VueQueryPlugin, { queryClient }]] } },
  )
  return { api, code, harness }
}

async function start(at: string, code: string, basePath?: string) {
  const router = makeRouter()
  await router.push(at)
  await router.isReady()
  const { api, code: codeRef, harness } = mountGrouping(router, code, basePath)
  await nextTick()
  return { router, api, code: codeRef, harness }
}

const query = (router: Router) => router.currentRoute.value.query

describe('useSetGrouping', () => {
  it('derives the group scope for a sub-set', async () => {
    const { api } = await start('/cards/mtg/sets/blc', 'blc')
    expect(api.group.value?.main.code).toBe('blb')
    expect(api.isMainSet.value).toBe(false)
    expect(api.relatedCount.value).toBe(1)
    expect(api.hasRelated.value).toBe(true)
    // Menu offers the main set (full name) and the sub-set (parent prefix stripped).
    expect(api.memberOptions.value).toEqual([
      { code: 'blb', name: 'Bloomburrow', label: 'Bloomburrow' },
      { code: 'blc', name: 'Bloomburrow Commander', label: 'Commander' },
    ])
  })

  it('remembers the origin set across a sub-set → view-all → view-single round-trip', async () => {
    // Enter the grouped view from the sub-set: it roots at the main set but remembers
    // where we came from via ?from, so "view just this set" can return there.
    const { router, api, code } = await start('/cards/mtg/sets/blc', 'blc')
    api.setIncludeRelated(true)
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/cards/mtg/sets/blb')
    expect(query(router).related).toBe('1')
    expect(query(router).from).toBe('blc')

    // Mirror the view re-rendering on the main set after the navigation.
    code.value = 'blb'
    await nextTick()
    expect(api.includeRelated.value).toBe(true)
    expect(api.activeSetCode.value).toBeNull()
    // The origin (the sub-set we came from) is remembered, not the main set.
    expect(api.originName.value).toBe('Bloomburrow Commander')

    // Leaving the grouped view returns to that origin sub-set, not the parent.
    api.setIncludeRelated(false)
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/cards/mtg/sets/blc')
    expect(query(router).related).toBeUndefined()
    expect(query(router).from).toBeUndefined()
  })

  it('toggles the related scope in place on the main set and preserves search + sort', async () => {
    const { router, api } = await start('/cards/mtg/sets/blb?q=elf&sort=name:desc', 'blb')
    expect(api.isMainSet.value).toBe(true)

    // View-all on the main set stays on the same path, just flips ?related on while
    // carrying the search + sort across (paging is dropped).
    api.setIncludeRelated(true)
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/cards/mtg/sets/blb')
    expect(query(router).related).toBe('1')
    expect(query(router).q).toBe('elf')
    expect(query(router).sort).toBe('name:desc')
    expect(api.includeRelated.value).toBe(true)
  })

  it('routes to a different member set fresh, dropping the current scope', async () => {
    const { router, api } = await start('/cards/mtg/sets/blb?related=1', 'blb')
    api.viewSingleSet('blc')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/cards/mtg/sets/blc')
    expect(query(router).related).toBeUndefined()
  })

  it('navigates under a custom basePath (the collection reuse)', async () => {
    // The collection set view passes basePath '/collection', so the scope nav must route
    // to the collection's own set pages, not the catalog's.
    const { router, api } = await start('/collection/mtg/sets/blc', 'blc', '/collection')
    api.setIncludeRelated(true)
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/collection/mtg/sets/blb')
    expect(query(router).related).toBe('1')
    expect(query(router).from).toBe('blc')

    api.viewSingleSet('blb')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/collection/mtg/sets/blb')
  })
})
