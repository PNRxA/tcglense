import { describe, it, expect, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import ProductCards from '../ProductCards.vue'
import { PRODUCT_CARDS_MODAL_SEARCH_KEYS } from '@/composables/useProductCardsSearch'

// The #448 fix lives in two halves that the other suites each cover alone: the composable spec
// drives a hand-rolled id ref (simulating the prop change), and ProductCards.spec.ts mocks the
// composable wholesale. Neither notices ProductCards passing a NON-reactive id (say, a plain
// `props.id` instead of the toRef) — which type-checks, keeps every other test green, and
// silently reintroduces the bug. This suite pins the production wiring: the real component,
// the real composable, and an id prop stepped the way DetailDialogShell's slot steps it.
vi.mock('@/composables/useProducts', () => ({
  useProductCardSectionsQuery: () => ({
    data: { value: { data: [{ key: 'booster', total: 1, booster_family: null }] } },
    error: { value: undefined },
  }),
}))

// Only the search box needs to be real (it's how the test types); the section blocks and the
// toolbar menus carry their own query/store wiring and are out of scope here.
async function mountAt(url: string) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/sealed/:game', component: { template: '<div />' } },
      { path: '/sealed/:game/:id', component: { template: '<div />' } },
    ],
  })
  await router.push(url)
  await router.isReady()
  const wrapper = mount(ProductCards, {
    props: {
      game: 'mtg',
      id: '100',
      productType: 'bundle',
      searchKeys: PRODUCT_CARDS_MODAL_SEARCH_KEYS,
    },
    global: {
      plugins: [router],
      stubs: {
        ProductCardsSection: true,
        AdvancedSearchPanel: true,
        SearchSyntaxHint: true,
        CardSizeMenu: true,
        CardSortMenu: true,
      },
    },
  })
  await flushPromises()
  return { wrapper, router }
}

const input = (wrapper: Awaited<ReturnType<typeof mountAt>>['wrapper']) =>
  wrapper.get('input').element as HTMLInputElement

describe('ProductCards search wiring (id prop → composable resync, #448)', () => {
  it('cancels a half-typed search when the id prop steps to another product', async () => {
    const { wrapper, router } = await mountAt('/sealed/mtg?product=100')
    await wrapper.get('input').setValue('go')
    await flushPromises() // still inside the 300ms debounce — nothing committed yet

    // A modal step: DetailDialogShell rewrites ?product= (an explicit fresh query, as goTo
    // leaves it) and the slot re-renders this component with the new id — the path never moves.
    await router.replace({ query: { product: '200' } })
    await wrapper.setProps({ id: '200' })
    await flushPromises()
    expect(input(wrapper).value).toBe('')

    // The pending 'go' must never land on product 200.
    await new Promise((resolve) => setTimeout(resolve, 330))
    await flushPromises()
    expect(router.currentRoute.value.query.pq).toBeUndefined()
    expect(router.currentRoute.value.query.product).toBe('200')
  })

  it('hydrates the box from a deep-linked ?pq= through the threaded modal keys', async () => {
    const { wrapper } = await mountAt('/sealed/mtg?product=100&pq=t:goblin')
    expect(input(wrapper).value).toBe('t:goblin')
  })
})
