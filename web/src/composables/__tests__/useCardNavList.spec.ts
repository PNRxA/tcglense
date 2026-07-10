import { afterEach, describe, expect, it } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Card, CollectionEntry } from '@/lib/api'
import CardGrid from '@/components/cards/CardGrid.vue'
import CollectionGrid from '@/components/collection/CollectionGrid.vue'
import { useCardNavStore } from '@/stores/cardNav'

// A minimal Card — only the fields the grid/tile touch matter for these tests.
function makeCard(id: string): Card {
  return {
    id,
    name: `Card ${id}`,
    set_code: 'tst',
    set_name: 'TST',
    collector_number: '1',
    rarity: 'rare',
    lang: 'en',
    released_at: '2024-01-01',
    mana_cost: '{2}',
    cmc: 2,
    type_line: 'Artifact',
    oracle_text: null,
    power: null,
    toughness: null,
    loyalty: null,
    color_identity: [],
    colors: [],
    layout: 'normal',
    prices: { usd: null, usd_foil: null, eur: null, tix: null },
    has_image: false,
    drop_name: null,
    drop_slug: null,
    secret_lair_bonus: false,
    secret_lair_spend_incentive: false,
    faces: [],
  }
}

function entry(id: string): CollectionEntry {
  return { card: makeCard(id), quantity: 1, foil_quantity: 0 }
}

let wrapper: VueWrapper

// The tree needs a router (CardTile renders a link), Pinia (card-size + this feature's nav
// store), and vue-query (the quick-add controls) — same as CardGrid's own suite. The plugins
// tuple is built inline at each mount so its shape stays inferable.
function makeDeps() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/cards/:game/cards/:id', component: { template: '<div />' } }],
  })
  const pinia = createPinia()
  setActivePinia(pinia)
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return { router, pinia, queryClient }
}

describe('useCardNavList (grid → nav registry bridge)', () => {
  afterEach(() => {
    wrapper?.unmount()
  })

  it('publishes a CardGrid’s ids so the modal can find neighbours', () => {
    const { router, pinia, queryClient } = makeDeps()
    wrapper = mount(CardGrid, {
      props: { game: 'mtg', cards: [makeCard('a'), makeCard('b'), makeCard('c')] },
      global: { plugins: [router, pinia, [VueQueryPlugin, { queryClient }]] },
    })
    const nav = useCardNavStore()
    expect(nav.locate('mtg', 'b')).toEqual({ prev: 'a', next: 'c', index: 1, total: 3 })
  })

  it('keys a CollectionGrid off entry.card.id', () => {
    const { router, pinia, queryClient } = makeDeps()
    wrapper = mount(CollectionGrid, {
      props: { game: 'mtg', entries: [entry('a'), entry('b')] },
      global: { plugins: [router, pinia, [VueQueryPlugin, { queryClient }]] },
    })
    const nav = useCardNavStore()
    expect(nav.locate('mtg', 'a')).toEqual({ prev: null, next: 'b', index: 0, total: 2 })
  })

  it('follows the grid when its cards change (a page change)', async () => {
    const { router, pinia, queryClient } = makeDeps()
    wrapper = mount(CardGrid, {
      props: { game: 'mtg', cards: [makeCard('a'), makeCard('b')] },
      global: { plugins: [router, pinia, [VueQueryPlugin, { queryClient }]] },
    })
    const nav = useCardNavStore()
    expect(nav.locate('mtg', 'a').total).toBe(2)

    await wrapper.setProps({ cards: [makeCard('x'), makeCard('y'), makeCard('z')] })
    expect(nav.locate('mtg', 'a')).toEqual({ prev: null, next: null, index: -1, total: 0 })
    expect(nav.locate('mtg', 'y')).toEqual({ prev: 'x', next: 'z', index: 1, total: 3 })
  })

  it('withdraws its ids on unmount', () => {
    const { router, pinia, queryClient } = makeDeps()
    wrapper = mount(CardGrid, {
      props: { game: 'mtg', cards: [makeCard('a'), makeCard('b')] },
      global: { plugins: [router, pinia, [VueQueryPlugin, { queryClient }]] },
    })
    const nav = useCardNavStore()
    expect(nav.locate('mtg', 'a').index).toBe(0)

    wrapper.unmount()
    expect(nav.locate('mtg', 'a')).toEqual({ prev: null, next: null, index: -1, total: 0 })
  })
})
