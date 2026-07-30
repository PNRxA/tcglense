import { describe, it, expect, vi, beforeAll } from 'vitest'

import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Game } from '@/lib/api'
import { allNavItems, itemWarmTargets, resolveItem } from '@/lib/nav'
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
      { path: '/keywords', component: { template: '<div />' } },
      { path: '/keywords/:game', component: { template: '<div />' } },
      { path: '/decks', component: { template: '<div />' } },
      { path: '/decks/:game', component: { template: '<div />' } },
      { path: '/tools', component: { template: '<div />' } },
      { path: '/tools/:game', component: { template: '<div />' } },
      { path: '/tools/:game/:tool', component: { template: '<div />' } },
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

  it('reveals every registry destination when opened', async () => {
    const wrapper = await mountNav([MTG])
    await openDrawer(wrapper)

    // Registry-driven on purpose: a hand-written href list here would be a fifth copy of
    // the IA — exactly what `lib/nav.ts` exists to end — and would go stale the moment a
    // destination is added. `itemWarmTargets(resolveItem(...))` is the same flattening the
    // drawer's template and its prefetch warming both derive from, so this pins that every
    // resolved link actually reaches the DOM.
    //
    // The drawer content teleports to the body, so query the whole document, not the
    // wrapper — and note `/scan` and `/docs` render in the pinned footer rather than the
    // scrolling region, which is why this sweeps the drawer as a whole.
    const hrefs = Array.from(document.querySelectorAll('a')).map((a) => a.getAttribute('href'))
    for (const item of allNavItems()) {
      for (const to of itemWarmTargets(resolveItem(item, [MTG]))) {
        expect(hrefs, `${item.id} → ${to}`).toContain(to)
      }
    }

    wrapper.unmount()
  })

  it('has no Alerts link — it moved to UserMenu', async () => {
    // The one deliberate removal in the nav consolidation: /alerts is account-scoped
    // notification *settings*, so it lives in UserMenu, which renders at every width
    // (App.vue, outside the `lg` gate). Named failing test rather than a silent absence,
    // because re-adding it by reflex is the likely mistake.
    const wrapper = await mountNav([MTG])
    await openDrawer(wrapper)

    expect(document.querySelector('a[href="/alerts"]')).toBeNull()
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
