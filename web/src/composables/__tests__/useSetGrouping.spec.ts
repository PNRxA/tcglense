import { describe, it, expect } from 'vitest'

import { defineComponent, h, nextTick, ref, type Ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import type { CardSet } from '@/lib/api'
import { makeCardSet } from '@/test/fixtures'
import { useSetGrouping } from '../useSetGrouping'

// A main set (Bloomburrow) with one related sub-set (its Commander decks). findGroup
// resolves both codes to this same group, which is all the scope nav needs.
const SETS: CardSet[] = [
  makeCardSet('blb', { name: 'Bloomburrow' }),
  makeCardSet('blc', {
    name: 'Bloomburrow Commander',
    parent_set_code: 'blb',
    set_type: 'commander',
  }),
  // A drop-grouped set (Secret Lair) with one related sub-set, for the by-drop tests.
  makeCardSet('sld', { name: 'Secret Lair Drop', has_drops: true }),
  makeCardSet('sls', {
    name: 'Secret Lair Showdown',
    parent_set_code: 'sld',
    set_type: 'commander',
  }),
  // A set with special treatments (borderless, showcase, …) for the by-sub-type tests.
  makeCardSet('woe', { name: 'Wilds of Eldraine', has_subtypes: true }),
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
function mountGrouping(
  router: Router,
  initialCode: string,
  options: { basePath?: string; preserveQuery?: string[] } = {},
) {
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
        api = useSetGrouping(game, code, options)
        return () => h('div')
      },
    }),
    { global: { plugins: [router, [VueQueryPlugin, { queryClient }]] } },
  )
  return { api, code, harness }
}

async function start(
  at: string,
  code: string,
  options: { basePath?: string; preserveQuery?: string[] } = {},
) {
  const router = makeRouter()
  await router.push(at)
  await router.isReady()
  const { api, code: codeRef, harness } = mountGrouping(router, code, options)
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
    // The bundle the views v-bind onto SetScopeBar mirrors those derivations (the
    // origin falls back to the main set when there's no ?from).
    expect(api.scopeBarProps.value).toEqual({
      includeRelated: false,
      isMainSet: false,
      mainName: 'Bloomburrow',
      relatedCount: 1,
      setsWord: 'set',
      memberOptions: api.memberOptions.value,
      activeSetCode: 'blc',
      originName: 'Bloomburrow',
    })
  })

  it('stays inert for the unscoped collection view (empty code)', async () => {
    // The collection's all-cards view passes code '' — it resolves to no group and no
    // grouping, so the scope bar and grouped view stay off without a scoped guard in the view.
    const { api } = await start('/', '')
    expect(api.hasRelated.value).toBe(false)
    expect(api.hasDrops.value).toBe(false)
    expect(api.groupMode.value).toBeNull()
    expect(api.grouped.value).toBe(false)
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
    const { router, api } = await start('/collection/mtg/sets/blc', 'blc', {
      basePath: '/collection',
    })
    api.setIncludeRelated(true)
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/collection/mtg/sets/blb')
    expect(query(router).related).toBe('1')
    expect(query(router).from).toBe('blc')

    api.viewSingleSet('blb')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/collection/mtg/sets/blb')
  })

  it('carries a preserved query key (ghosts) across every scope-nav', async () => {
    // The collection's show-ghosts mode is orthogonal to the include-related scope, so it
    // must survive toggling it — from a sub-set (fresh nav to the main set), back to a
    // single set, and jumping to a different member.
    const { router, api, code } = await start('/collection/mtg/sets/blc?ghosts=1', 'blc', {
      basePath: '/collection',
      preserveQuery: ['ghosts'],
    })

    // Sub-set → "view all together": fresh nav to the main set keeps ghosts.
    api.setIncludeRelated(true)
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/collection/mtg/sets/blb')
    expect(query(router).related).toBe('1')
    expect(query(router).ghosts).toBe('1')

    code.value = 'blb'
    await nextTick()

    // "View just this set" (toggle off) back to the origin sub-set keeps ghosts.
    api.setIncludeRelated(false)
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/collection/mtg/sets/blc')
    expect(query(router).related).toBeUndefined()
    expect(query(router).ghosts).toBe('1')

    // Mirror the view re-rendering on the origin set after that navigation, then jump to a
    // different member set: it keeps ghosts (a view preference) while shedding the scope.
    code.value = 'blc'
    await nextTick()
    api.viewSingleSet('blb')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/collection/mtg/sets/blb')
    expect(query(router).ghosts).toBe('1')
  })

  it('drives the grouped view off the grouping, ?view=all and the related scope', async () => {
    // A drop-grouped set defaults to grouped (by drop); a plain set never activates it.
    const { router, api } = await start('/cards/mtg/sets/sld', 'sld')
    expect(api.hasDrops.value).toBe(true)
    expect(api.groupMode.value).toBe('drops')
    expect(api.groupLabel.value).toBe('By drop')
    expect(api.grouped.value).toBe(true)

    // ?view=all opts back into the flat grid.
    await router.replace({ query: { view: 'all' } })
    expect(api.grouped.value).toBe(false)

    // The related-sets view is itself a flat cross-set listing, so it suppresses grouping.
    await router.replace({ query: { related: '1' } })
    expect(api.includeRelated.value).toBe(true)
    expect(api.grouped.value).toBe(false)
  })

  it('drives the by-sub-type view off has_subtypes (drops take precedence)', async () => {
    // A set with treatments but no drops groups by sub-type, labelled "By treatment".
    const { api } = await start('/cards/mtg/sets/woe', 'woe')
    expect(api.hasSubtypes.value).toBe(true)
    expect(api.hasDrops.value).toBe(false)
    expect(api.groupMode.value).toBe('subtypes')
    expect(api.groupLabel.value).toBe('By treatment')
    expect(api.grouped.value).toBe(true)
  })

  it('toggles the grouped vs flat view, keeping search + sort and restarting paging', async () => {
    const { router, api } = await start('/cards/mtg/sets/sld?q=elf&sort=name:desc&page=3', 'sld')
    api.setGroupView('all')
    await flushPromises()
    expect(query(router).view).toBe('all')
    expect(query(router).q).toBe('elf')
    expect(query(router).sort).toBe('name:desc')
    expect(query(router).page).toBeUndefined()
    expect(api.grouped.value).toBe(false)

    // Back to grouped: ?view is shed (grouped is the bare default).
    api.setGroupView('grouped')
    await flushPromises()
    expect(query(router).view).toBeUndefined()
    expect(query(router).q).toBe('elf')
    expect(api.grouped.value).toBe(true)
  })

  it('carries a preserved query key (ghosts) across the grouped toggle', async () => {
    const { router, api } = await start('/collection/mtg/sets/sld?ghosts=1', 'sld', {
      basePath: '/collection',
      preserveQuery: ['ghosts'],
    })
    api.setGroupView('all')
    await flushPromises()
    expect(query(router).view).toBe('all')
    expect(query(router).ghosts).toBe('1')
  })
})
