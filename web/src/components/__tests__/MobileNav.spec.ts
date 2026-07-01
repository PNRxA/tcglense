import { describe, it, expect, vi, beforeAll } from 'vitest'

import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Game } from '@/lib/api'
import MobileNav from '../MobileNav.vue'

// reka-ui's menu primitives lean on ResizeObserver (for positioning), which jsdom
// doesn't implement — stub it so opening the menu doesn't throw.
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
  // Attach to the document so the dropdown's teleported content is queryable.
  return mount(MobileNav, {
    attachTo: document.body,
    global: { plugins: [router, [VueQueryPlugin, { queryClient }]] },
  })
}

const MTG: Game = {
  id: 'mtg',
  name: 'Magic: The Gathering',
  publisher: 'Wizards',
  data_source: 'scryfall',
}

describe('MobileNav', () => {
  it('renders an accessible hamburger trigger', async () => {
    const wrapper = await mountNav()
    const trigger = wrapper.find('button[aria-label="Open navigation menu"]')
    expect(trigger.exists()).toBe(true)
    wrapper.unmount()
  })

  it('reveals both catalog and collection links when opened', async () => {
    const wrapper = await mountNav([MTG])
    await wrapper.find('button[aria-label="Open navigation menu"]').trigger('click')
    await flushPromises()

    // The menu content teleports to the body, so query the document, not the wrapper.
    const hrefs = Array.from(document.querySelectorAll('a')).map((a) => a.getAttribute('href'))
    expect(hrefs).toContain('/cards')
    expect(hrefs).toContain('/cards/mtg')
    expect(hrefs).toContain('/collection')
    expect(hrefs).toContain('/collection/mtg')

    wrapper.unmount()
  })
})
