import { describe, it, expect, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Card, Product, ProductComponent } from '@/lib/api'
import ProductContents from '../ProductContents.vue'

// Drive the component off a controlled composition, stubbing the query composable so no
// QueryClient is needed — the unit under test is which line items render, their quantity + name,
// which ones link, and that a link opens the shared detail modal in place (issue #485) rather
// than navigating away.
const state = vi.hoisted(() => ({ components: [] as ProductComponent[] }))

vi.mock('@/composables/useProducts', () => ({
  useProductContentsQuery: () => ({
    data: {
      get value() {
        return { data: state.components }
      },
    },
  }),
}))

// Minimal component/product/card factories carrying only the fields the template reads.
function comp(overrides: Partial<ProductComponent> = {}): ProductComponent {
  return { kind: 'other', name: 'Extra', quantity: 1, product: null, card: null, ...overrides }
}
function linkedProduct(id: string, hasImage = true): Product {
  return { id, name: `Product ${id}`, has_image: hasImage } as unknown as Product
}
function linkedCard(id: string, hasImage = false): Card {
  return { id, name: `Card ${id}`, has_image: hasImage } as unknown as Card
}

// The section renders on a real sealed-product page, so a plain click opening a nested item's
// modal lands `?product=`/`?card=` over the current path — exactly what happens inside the
// browse-grid product modal too.
async function mountContents(components: ProductComponent[], path = '/sealed/mtg/parent') {
  state.components = components
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/sealed/:game', component: { template: '<div />' } },
      { path: '/sealed/:game/:id', component: { template: '<div />' } },
      { path: '/cards/:game/cards/:id', component: { template: '<div />' } },
    ],
  })
  await router.push(path)
  await router.isReady()
  const wrapper = mount(ProductContents, {
    props: { game: 'mtg', id: 'parent' },
    global: { plugins: [router] },
  })
  return { wrapper, router }
}

describe('ProductContents', () => {
  it('renders nothing when the product has no composition', async () => {
    const { wrapper } = await mountContents([])
    expect(wrapper.find('h2').exists()).toBe(false)
    expect(wrapper.findAll('a')).toHaveLength(0)
  })

  it('lists each component with its quantity and name, in order', async () => {
    const { wrapper } = await mountContents([
      comp({ kind: 'sealed', name: 'Play Booster', quantity: 9, product: linkedProduct('648640') }),
      comp({ kind: 'other', name: 'Card storage box', quantity: 1 }),
    ])
    // Heading carries the section title plus a live item count (SEO enrichment, issue #302).
    expect(wrapper.find('h2').text()).toContain("What's in the box")
    expect(wrapper.find('h2').text()).toContain('2 items')
    const items = wrapper.findAll('li')
    expect(items).toHaveLength(2)
    expect(items[0]!.text()).toContain('9×')
    expect(items[0]!.text()).toContain('Play Booster')
    expect(items[1]!.text()).toContain('Card storage box')
  })

  it('links sealed and card components to their canonical pages; textual extras are not links', async () => {
    const { wrapper } = await mountContents([
      comp({ kind: 'sealed', name: 'Play Booster', quantity: 9, product: linkedProduct('648640') }),
      comp({ kind: 'card', name: 'Sol Ring', quantity: 1, card: linkedCard('sf-sol') }),
      comp({ kind: 'other', name: 'Storage box', quantity: 1 }),
    ])
    const links = wrapper.findAll('a')
    // Only the two resolvable components are anchors; the textual extra is a plain <div>. The
    // anchors keep the canonical full page as their href for modifier/middle clicks + crawlers.
    expect(links).toHaveLength(2)
    expect(links[0]!.attributes('href')).toBe('/sealed/mtg/648640')
    expect(links[1]!.attributes('href')).toBe('/cards/mtg/cards/sf-sol')
    expect(wrapper.findAll('li')[2]!.find('a').exists()).toBe(false)
  })

  it('opens a nested product in the sealed-product modal in place, keeping the page', async () => {
    const { wrapper, router } = await mountContents([
      comp({ kind: 'sealed', name: 'Play Booster', quantity: 9, product: linkedProduct('648640') }),
    ])
    await wrapper.get('a').trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/sealed/mtg/parent')
    expect(router.currentRoute.value.query.product).toBe('648640')
    // On the full page the parent is the page (not a modal), so there's nothing to go "back" to.
    expect(router.currentRoute.value.query.openedFrom).toBeUndefined()
  })

  it('remembers the parent product for the "back" crumb when opened inside the product modal', async () => {
    // When this section renders inside the browse-grid product modal (?product=parent), clicking a
    // nested pack swaps to it AND stashes the parent so the modal shows "← Back to <parent>" (#485).
    const { wrapper, router } = await mountContents(
      [
        comp({
          kind: 'sealed',
          name: 'Play Booster',
          quantity: 9,
          product: linkedProduct('648640'),
        }),
      ],
      '/sealed/mtg?product=parent',
    )
    await wrapper.get('a').trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.query.product).toBe('648640')
    expect(router.currentRoute.value.query.openedFrom).toBe('product:parent')
  })

  it('opens a promo card in the card modal, remembering the product for the "back" crumb', async () => {
    // Start with the parent product modal open (as when this section renders inside the
    // browse-grid product modal): clicking a promo card swaps surfaces and stashes the origin.
    const { wrapper, router } = await mountContents(
      [comp({ kind: 'card', name: 'Sol Ring', quantity: 1, card: linkedCard('sf-sol') })],
      '/sealed/mtg?product=648640',
    )
    await wrapper.get('a').trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.query.product).toBeUndefined()
    expect(router.currentRoute.value.query.card).toBe('sf-sol')
    expect(router.currentRoute.value.query.openedFrom).toBe('product:648640')
  })

  it('leaves modifier-click navigation to the browser', async () => {
    const { wrapper, router } = await mountContents([
      comp({ kind: 'sealed', name: 'Play Booster', quantity: 9, product: linkedProduct('648640') }),
    ])
    const push = vi.spyOn(router, 'push')
    wrapper.get('a').element.addEventListener('click', (event) => event.preventDefault(), {
      once: true,
    })
    await wrapper.get('a').trigger('click', { ctrlKey: true })
    expect(push).not.toHaveBeenCalled()
  })

  it('shows a thumbnail only for links with art, else the icon fallback', async () => {
    const { wrapper } = await mountContents([
      comp({ kind: 'sealed', name: 'With art', quantity: 1, product: linkedProduct('1', true) }),
      comp({ kind: 'sealed', name: 'No art', quantity: 1, product: linkedProduct('2', false) }),
      comp({ kind: 'card', name: 'Card art', quantity: 1, card: linkedCard('c1', true) }),
      comp({ kind: 'card', name: 'Card no art', quantity: 1, card: linkedCard('c2', false) }),
      comp({ kind: 'other', name: 'Die', quantity: 1 }),
    ])
    const items = wrapper.findAll('li')
    // Only the art-bearing product + card render an <img>; art-less links + the textual
    // extra fall back to the kind icon (never a broken image).
    expect(items[0]!.find('img').exists()).toBe(true)
    expect(items[1]!.find('img').exists()).toBe(false)
    expect(items[2]!.find('img').exists()).toBe(true)
    expect(items[3]!.find('img').exists()).toBe(false)
    expect(items[4]!.find('img').exists()).toBe(false)
  })
})
