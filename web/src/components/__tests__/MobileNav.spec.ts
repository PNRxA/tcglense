import { describe, it, expect, vi, beforeAll } from 'vitest'

import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Game } from '@/lib/api'
import MobileNav from '../MobileNav.vue'

// reka-ui's primitives lean on ResizeObserver (for positioning), which jsdom
// doesn't implement — stub it so opening the drawer doesn't throw.
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
      { path: '/wishlist', component: { template: '<div />' } },
      { path: '/wishlist/:game', component: { template: '<div />' } },
      { path: '/scan', component: { template: '<div />' } },
      { path: '/docs', component: { template: '<div />' } },
    ],
  })
}

async function mountNav(games: Game[] = [], startAt = '/') {
  const router = makeRouter()
  router.push(startAt)
  await router.isReady()
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  // Seed the cache so `games` is populated synchronously (no network in tests).
  queryClient.setQueryData(['games'], { data: games })
  // Attach to the document so the drawer's teleported content is queryable.
  return mount(MobileNav, {
    attachTo: document.body,
    global: { plugins: [router, [VueQueryPlugin, { queryClient }]] },
  })
}

async function openDrawer(wrapper: Awaited<ReturnType<typeof mountNav>>) {
  await wrapper.find('button[aria-label="Open navigation menu"]').trigger('click')
  await flushPromises()
}

// Clicks a teleported anchor the way a real tap does. jsdom's HTMLElement.click()
// would navigate; dispatching a plain (non-modified, left-button) click bubbles to the
// drawer's delegated handler AND is handled by RouterLink.
async function clickAnchor(href: string) {
  const anchor = Array.from(document.querySelectorAll('a')).find(
    (a) => a.getAttribute('href') === href,
  )
  expect(anchor, `anchor ${href} should be in the open drawer`).toBeTruthy()
  anchor!.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0 }))
  await flushPromises()
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

  it('reveals catalog, sealed, collection and wish-list links when opened', async () => {
    const wrapper = await mountNav([MTG])
    await openDrawer(wrapper)

    // The drawer content teleports to the body, so query the document, not the wrapper.
    // The section titles are themselves the landing anchors now (no "Browse all games"
    // rows); Scan cards and API docs live in the pinned footer.
    const hrefs = Array.from(document.querySelectorAll('a')).map((a) => a.getAttribute('href'))
    expect(hrefs).toContain('/cards')
    expect(hrefs).toContain('/cards/mtg')
    expect(hrefs).toContain('/sealed')
    expect(hrefs).toContain('/sealed/mtg')
    expect(hrefs).toContain('/collection')
    expect(hrefs).toContain('/collection/mtg')
    expect(hrefs).toContain('/wishlist')
    expect(hrefs).toContain('/wishlist/mtg')
    expect(hrefs).toContain('/scan')
    expect(hrefs).toContain('/docs')

    wrapper.unmount()
  })

  it('closes the drawer when a link is clicked', async () => {
    const wrapper = await mountNav([MTG])
    await openDrawer(wrapper)
    expect(document.querySelector('[role="dialog"]')).toBeTruthy()

    await clickAnchor('/cards/mtg')

    // The Sheet is a dialog and does not auto-close on link activation — the component's
    // delegated click handler (plus a route watcher) must close it.
    expect(document.querySelector('[role="dialog"]')).toBeNull()
    wrapper.unmount()
  })

  it('closes the drawer when tapping the already-active route', async () => {
    // Start ON /cards: clicking its own link fires no route change, so the route
    // watcher alone would leave the drawer stuck open — only the delegated click
    // handler closes it. This is the regression a watcher-only rewrite would cause.
    const wrapper = await mountNav([MTG], '/cards')
    await openDrawer(wrapper)
    expect(document.querySelector('[role="dialog"]')).toBeTruthy()

    await clickAnchor('/cards')

    expect(document.querySelector('[role="dialog"]')).toBeNull()
    wrapper.unmount()
  })
})
