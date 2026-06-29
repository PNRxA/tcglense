import { describe, it, expect, vi, beforeAll } from 'vitest'

import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Game } from '@/lib/api'
import CardsNav from '../CardsNav.vue'

// reka-ui's navigation-menu viewport measures its content with ResizeObserver, which
// jsdom doesn't implement — stub it so opening the menu doesn't throw.
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
  return mount(CardsNav, {
    global: { plugins: [router, [VueQueryPlugin, { queryClient }]] },
  })
}

describe('CardsNav', () => {
  it('renders the Cards menu trigger', async () => {
    const wrapper = await mountNav()
    const trigger = wrapper.find('button')
    expect(trigger.exists()).toBe(true)
    expect(trigger.text()).toContain('Cards')
  })

  it('reveals a browse-all link and one shortcut per game when opened', async () => {
    const wrapper = await mountNav([
      { id: 'mtg', name: 'Magic: The Gathering', publisher: 'Wizards', data_source: 'scryfall' },
    ])
    await wrapper.find('button').trigger('click')
    await flushPromises()

    const browseAll = wrapper.find('a[href="/cards"]')
    expect(browseAll.exists()).toBe(true)
    expect(browseAll.text()).toContain('Browse all games')

    const gameLink = wrapper.find('a[href="/cards/mtg"]')
    expect(gameLink.exists()).toBe(true)
    expect(gameLink.text()).toContain('Magic: The Gathering')
  })
})
