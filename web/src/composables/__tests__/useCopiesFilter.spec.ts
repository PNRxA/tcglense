import { describe, it, expect } from 'vitest'

import { defineComponent, h, nextTick } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { useCopiesFilter } from '../useCopiesFilter'

function makeRouter() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/collection/:game/cards', name: 'cards', component: { template: '<div />' } },
      { path: '/collection/:game/sets/:code', name: 'set', component: { template: '<div />' } },
    ],
  })
}

// A throwaway harness so useRoute/useRouter resolve; it lives outside <RouterView>, so
// navigating doesn't unmount it (same pattern as useCardSearch's spec).
async function start(at: string) {
  const router = makeRouter()
  await router.push(at)
  await router.isReady()
  let api!: ReturnType<typeof useCopiesFilter>
  mount(
    defineComponent({
      setup() {
        api = useCopiesFilter()
        return () => h('div')
      },
    }),
    { global: { plugins: [router] } },
  )
  await nextTick()
  return { router, api }
}

const query = (router: Router) => router.currentRoute.value.query

describe('useCopiesFilter', () => {
  it('is inactive and unbounded with no filter in the URL', async () => {
    const { api } = await start('/collection/mtg/cards')
    expect(api.copies.value).toEqual({ finish: 'any' })
    expect(api.active.value).toBe(false)
  })

  it('hydrates the bounds and finish from the URL', async () => {
    const { api } = await start('/collection/mtg/cards?copies=2-3&finish=foil')
    expect(api.copies.value).toEqual({ min: 2, max: 3, finish: 'foil' })
    expect(api.active.value).toBe(true)
  })

  it('is active on a finish alone — "cards I hold any foil of"', async () => {
    const { api } = await start('/collection/mtg/cards?finish=foil')
    expect(api.copies.value).toEqual({ finish: 'foil' })
    expect(api.active.value).toBe(true)
  })

  it('ignores an unparseable ?copies= and an unknown ?finish= rather than filtering on junk', async () => {
    const { api } = await start('/collection/mtg/cards?copies=five&finish=etched')
    expect(api.copies.value).toEqual({ finish: 'any' })
    expect(api.active.value).toBe(false)
  })

  it('commits a whole filter in one write and restarts paging', async () => {
    const { router, api } = await start('/collection/mtg/cards?page=4')
    api.set({ min: 5, finish: 'foil' })
    await flushPromises()
    expect(query(router)).toMatchObject({ copies: '5-', finish: 'foil' })
    expect(query(router).page).toBeUndefined()
    expect(api.copies.value).toEqual({ min: 5, finish: 'foil' })
  })

  it('spells the bounds canonically and drops the keys at their defaults', async () => {
    const { router, api } = await start('/collection/mtg/cards?copies=4&finish=foil')
    api.set({ min: 2, max: 3, finish: 'any' })
    await flushPromises()
    expect(query(router).copies).toBe('2-3')
    expect(query(router).finish).toBeUndefined()

    api.set({ max: 3, finish: 'regular' })
    await flushPromises()
    expect(query(router).copies).toBe('-3')
    expect(query(router).finish).toBe('regular')

    // Unbounded + default finish = no filter, no keys.
    api.set({ finish: 'any' })
    await flushPromises()
    expect(query(router).copies).toBeUndefined()
    expect(query(router).finish).toBeUndefined()
    expect(api.active.value).toBe(false)
  })

  it('replaces both halves at once, never leaving a stale key behind', async () => {
    // The two-write shape this replaced raced: the second `router.replace` snapshotted the
    // route before the first landed and re-added the key it had dropped.
    const { router, api } = await start('/collection/mtg/cards?copies=2-3&finish=foil')
    api.set({ min: 5, finish: 'any' })
    await flushPromises()
    expect(query(router).copies).toBe('5-')
    expect(query(router).finish).toBeUndefined()
  })

  it('clears both keys in one write', async () => {
    const { router, api } = await start('/collection/mtg/cards?copies=4&finish=foil&page=3')
    api.clear()
    await flushPromises()
    expect(query(router).copies).toBeUndefined()
    expect(query(router).finish).toBeUndefined()
    expect(query(router).page).toBeUndefined()
    expect(api.active.value).toBe(false)
  })

  it('leaves unrelated list keys alone — including the grouped view', async () => {
    // A grouped view is NOT flipped to the flat grid on commit (unlike a sort): the bounds
    // narrow the cards within each drop / sub-type just as well.
    const { router, api } = await start(
      '/collection/mtg/sets/sld?view=all&related=1&ghosts=1&q=elf',
    )
    api.set({ min: 4, max: 4, finish: 'any' })
    await flushPromises()
    expect(query(router).copies).toBe('4')
    expect(query(router).view).toBe('all')
    expect(query(router).related).toBe('1')
    expect(query(router).ghosts).toBe('1')
    expect(query(router).q).toBe('elf')
  })
})
