import { describe, it, expect, vi, beforeAll } from 'vitest'

import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Game } from '@/lib/api'
import { NAV, itemWarmTargets, resolveItem } from '@/lib/nav'
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
      { path: '/keywords', component: { template: '<div />' } },
      { path: '/keywords/:game', component: { template: '<div />' } },
      { path: '/collection', component: { template: '<div />' } },
      { path: '/collection/:game', component: { template: '<div />' } },
      { path: '/decks', component: { template: '<div />' } },
      { path: '/decks/:game', component: { template: '<div />' } },
      { path: '/scan', component: { template: '<div />' } },
      { path: '/wishlist', component: { template: '<div />' } },
      { path: '/wishlist/:game', component: { template: '<div />' } },
      { path: '/tools', component: { template: '<div />' } },
      { path: '/tools/:game', component: { template: '<div />' } },
      { path: '/tools/:game/:tool', component: { template: '<div />' } },
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

/** Whether any trigger button carries `label` — the negative half of the pin below. */
function hasTrigger(wrapper: Awaited<ReturnType<typeof mountNav>>, label: string) {
  return wrapper.findAll('button').some((b) => b.text().includes(label))
}

/** The menu roots, swept from the registry so a third one is covered without an edit. */
const MENU_ROOTS = NAV.filter((root) => root.kind === 'menu')

/** Every href a menu root's registry entry says it should render, for one game. */
function expectedHrefs(root: (typeof MENU_ROOTS)[number]): string[] {
  return root.groups.flatMap((group) =>
    group.items.flatMap((item) => itemWarmTargets(resolveItem(item, [MTG]))),
  )
}

async function openPanel(wrapper: Awaited<ReturnType<typeof mountNav>>, label: string) {
  await trigger(wrapper, label).trigger('click')
  await flushPromises()
}

describe('MainNav', () => {
  it('consolidates the top bar into Browse, Tools and a bare API link', async () => {
    const wrapper = await mountNav()
    expect(trigger(wrapper, 'Browse').exists()).toBe(true)
    expect(trigger(wrapper, 'Tools').exists()).toBe(true)

    // The user-visible half of the change: three catalog/library triggers became one.
    // Pinned explicitly, because "Browse exists" would still pass if the old triggers
    // had merely been left in place beside it.
    expect(hasTrigger(wrapper, 'Products')).toBe(false)
    expect(hasTrigger(wrapper, 'Collection')).toBe(false)
    expect(hasTrigger(wrapper, 'Wish list')).toBe(false)
    expect(wrapper.findAll('button[data-slot="navigation-menu-trigger"]')).toHaveLength(2)
  })

  it('links to the API docs directly (no dropdown)', async () => {
    const wrapper = await mountNav()
    const docs = wrapper.find('a[href="/docs"]')
    expect(docs.exists()).toBe(true)
    expect(docs.text()).toContain('API')
  })

  // The assertion that keeps this component honest. It compares the rendered anchors
  // against the registry's own resolved tree rather than a hand-written list of hrefs —
  // a fourth copy of the IA in a spec file would drift exactly the way the three
  // hand-written navs did. A destination added to `lib/nav.ts` is asserted here for
  // free; one silently dropped from the template fails here.
  for (const root of MENU_ROOTS) {
    it(`renders every registry destination under the ${root.label} panel`, async () => {
      const wrapper = await mountNav([MTG])
      await openPanel(wrapper, root.label)

      const hrefs = expectedHrefs(root)
      expect(hrefs.length).toBeGreaterThan(0)
      for (const href of hrefs) {
        expect(wrapper.find(`a[href="${href}"]`).exists(), `${root.id} → ${href}`).toBe(true)
      }
    })
  }

  it('splits the Browse panel into Catalog and Your library columns', async () => {
    const wrapper = await mountNav([MTG])
    await openPanel(wrapper, 'Browse')

    const panel = wrapper.find('[data-slot="navigation-menu-content"]')
    expect(panel.exists()).toBe(true)
    const columns = panel.findAll('ul')
    expect(columns).toHaveLength(2)
    expect(columns[0]!.text()).toContain('Catalog')
    expect(columns[1]!.text()).toContain('Your library')

    // The consolidation itself: catalog and library destinations now live in one panel,
    // in the column each belongs to.
    expect(columns[0]!.find('a[href="/cards"]').exists()).toBe(true)
    expect(columns[0]!.find('a[href="/keywords"]').exists()).toBe(true)
    expect(columns[1]!.find('a[href="/collection"]').exists()).toBe(true)
    expect(columns[1]!.find('a[href="/decks"]').exists()).toBe(true)
    expect(columns[1]!.find('a[href="/scan"]').exists()).toBe(true)
    // …and only in that column, or "two columns" would just be two copies of the panel.
    expect(columns[0]!.find('a[href="/collection"]').exists()).toBe(false)
    expect(columns[1]!.find('a[href="/cards"]').exists()).toBe(false)
  })

  it('names each landing row after its own item, so the rows are told apart', async () => {
    const wrapper = await mountNav([MTG])
    await openPanel(wrapper, 'Browse')

    // These rows used to read "Browse all games" / "All collections", which was fine while
    // each item had its own heading above it. Grouping the columns by Catalog / Your library
    // took those headings away and left three Catalog rows all saying "Browse all games" —
    // indistinguishable. The landing row carries the item's own name now, which is also
    // exactly what the drawer renders, so the two surfaces read the same.
    expect(wrapper.find('a[href="/cards"]').text()).toContain('Cards')
    expect(wrapper.find('a[href="/sealed"]').text()).toContain('Sealed products')
    expect(wrapper.find('a[href="/keywords"]').text()).toContain('Keyword glossary')
    expect(wrapper.find('a[href="/collection"]').text()).toContain('Collection')
    expect(wrapper.find('a[href="/decks"]').text()).toContain('Decks')
    // The per-game expansion under a landing is still one row per game.
    expect(wrapper.find('a[href="/cards/mtg"]').text()).toContain(MTG.name)
  })

  it('expands Tools to each tool plus its muted per-game index row', async () => {
    const wrapper = await mountNav([MTG])
    await openPanel(wrapper, 'Tools')

    expect(wrapper.find('a[href="/tools"]').text()).toContain('Tools')
    const life = wrapper.find('a[href="/tools/mtg/life"]')
    expect(life.exists()).toBe(true)
    expect(life.text()).toContain('Life counter')

    // `kind: 'index'` renders muted, so the "…and the rest" row reads as a footnote to
    // the tools above it rather than a peer of them.
    const index = wrapper.find('a[href="/tools/mtg"]')
    expect(index.text()).toContain(`All ${MTG.name} tools`)
    expect(index.classes()).toContain('text-muted-foreground')
  })
})
