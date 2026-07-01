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
      { path: '/collection', component: { template: '<div />' } },
      { path: '/collection/:game', component: { template: '<div />' } },
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
  it('renders both the Cards and Collection triggers in one menu', async () => {
    const wrapper = await mountNav()
    expect(trigger(wrapper, 'Cards').exists()).toBe(true)
    expect(trigger(wrapper, 'Collection').exists()).toBe(true)
  })

  it('reveals catalog links when the Cards menu is opened', async () => {
    const wrapper = await mountNav([MTG])
    await trigger(wrapper, 'Cards').trigger('click')
    await flushPromises()

    const browseAll = wrapper.find('a[href="/cards"]')
    expect(browseAll.exists()).toBe(true)
    expect(browseAll.text()).toContain('Browse all games')
    expect(wrapper.find('a[href="/cards/mtg"]').exists()).toBe(true)
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
})
