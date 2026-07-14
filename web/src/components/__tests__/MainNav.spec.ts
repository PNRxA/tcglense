import { describe, it, expect, vi, beforeAll } from 'vitest'

import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Game } from '@/lib/api'
import MainNav from '../MainNav.vue'

// reka-ui's navigation-menu viewport measures its content with ResizeObserver, which
// jsdom doesn't implement — stub it so opening a menu doesn't throw.
beforeAll(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
})

function makeRouter() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/cards', component: { template: '<div />' } },
      { path: '/cards/:game', component: { template: '<div />' } },
      { path: '/sealed', component: { template: '<div />' } },
      { path: '/sealed/:game', component: { template: '<div />' } },
      { path: '/collection', component: { template: '<div />' } },
      { path: '/collection/:game', component: { template: '<div />' } },
      { path: '/decks', component: { template: '<div />' } },
      { path: '/decks/:game', component: { template: '<div />' } },
      { path: '/scan', component: { template: '<div />' } },
      { path: '/wishlist', component: { template: '<div />' } },
      { path: '/wishlist/:game', component: { template: '<div />' } },
      { path: '/docs', component: { template: '<div />' } },
    ],
  })
}

async function mountNav(games: Game[] = []) {
  const router = makeRouter()
  router.push('/')
  await router.isReady()
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  // Seed the cache so `games` is populated synchronously (no network in tests).
  queryClient.setQueryData(['games'], { data: games })
  return mount(MainNav, {
    global: { plugins: [router, [VueQueryPlugin, { queryClient }]] },
  })
}

const MTG: Game = {
  id: 'mtg',
  name: 'Magic: The Gathering',
  publisher: 'Wizards',
  data_source: 'scryfall',
}

/** The trigger button whose text contains `label`. */
function trigger(wrapper: Awaited<ReturnType<typeof mountNav>>, label: string) {
  const button = wrapper.findAll('button').find((b) => b.text().includes(label))
  expect(button, `expected a "${label}" trigger`).toBeTruthy()
  return button!
}

describe('MainNav', () => {
  it('renders the Products, Collection and Wish list triggers in one menu', async () => {
    const wrapper = await mountNav()
    expect(trigger(wrapper, 'Products').exists()).toBe(true)
    expect(trigger(wrapper, 'Collection').exists()).toBe(true)
    expect(trigger(wrapper, 'Wish list').exists()).toBe(true)
  })

  it('links to the API docs directly (no dropdown)', async () => {
    const wrapper = await mountNav()
    const docs = wrapper.find('a[href="/docs"]')
    expect(docs.exists()).toBe(true)
    expect(docs.text()).toContain('API')
  })

  it('reveals both catalog and sealed links when the Products menu is opened', async () => {
    const wrapper = await mountNav([MTG])
    await trigger(wrapper, 'Products').trigger('click')
    await flushPromises()

    // Cards group.
    const browseCards = wrapper.find('a[href="/cards"]')
    expect(browseCards.exists()).toBe(true)
    expect(browseCards.text()).toContain('Browse all games')
    expect(wrapper.find('a[href="/cards/mtg"]').exists()).toBe(true)

    // Sealed group.
    expect(wrapper.find('a[href="/sealed"]').exists()).toBe(true)
    expect(wrapper.find('a[href="/sealed/mtg"]').exists()).toBe(true)
  })

  it('reveals collection links when the Collection menu is opened', async () => {
    const wrapper = await mountNav([MTG])
    await trigger(wrapper, 'Collection').trigger('click')
    await flushPromises()

    const all = wrapper.find('a[href="/collection"]')
    expect(all.exists()).toBe(true)
    expect(all.text()).toContain('All collections')
    expect(wrapper.find('a[href="/collection/mtg"]').exists()).toBe(true)
  })

  it('lists per-game deck links and the scan action under the Collection menu', async () => {
    const wrapper = await mountNav([MTG])
    await trigger(wrapper, 'Collection').trigger('click')
    await flushPromises()

    // Decks: an "all decks" landing plus one per-game link (issue #394), mirroring the
    // collections list above.
    const allDecks = wrapper.find('a[href="/decks"]')
    expect(allDecks.exists()).toBe(true)
    expect(allDecks.text()).toContain('All decks')
    const gameDecks = wrapper.find('a[href="/decks/mtg"]')
    expect(gameDecks.exists()).toBe(true)
    expect(gameDecks.text()).toContain(MTG.name)

    // Scan cards still lives in the same dropdown, below the decks.
    expect(wrapper.find('a[href="/scan"]').exists()).toBe(true)
  })

  it('reveals wish-list links when the Wish list menu is opened', async () => {
    const wrapper = await mountNav([MTG])
    await trigger(wrapper, 'Wish list').trigger('click')
    await flushPromises()

    const all = wrapper.find('a[href="/wishlist"]')
    expect(all.exists()).toBe(true)
    expect(all.text()).toContain('All wish lists')
    expect(wrapper.find('a[href="/wishlist/mtg"]').exists()).toBe(true)
  })
})
