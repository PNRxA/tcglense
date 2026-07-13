import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Card, CollectionEntry, OwnedCountsMap } from '@/lib/api'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'
import CollectionGrid from '../CollectionGrid.vue'

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

// One grid entry: a card plus its counts. On a collection grid those counts are collection
// holdings; on the wishlist surface they're wish-list wants (issue #167).
function entry(id: string, quantity = 1, foilQuantity = 0): CollectionEntry {
  return { card: makeCard(id), quantity, foil_quantity: foilQuantity }
}

// The wishlist page's DEFAULT (non-ghost) grids mount CollectionGrid; unlike CardGrid it has
// no auth gate — this surface is signed-in only, so every tile renders its quick-add control.
// The tree still needs a router (CardTile renders a link), Pinia (card-size store), and
// vue-query (the quick-add control), same as CardGrid's own suite.
function mountGrid(
  entries: CollectionEntry[],
  list: CardListTarget = 'collection',
  opts: {
    collectionCounts?: OwnedCountsMap
    ownedMarks?: OwnedCountsMap
    wishlist?: OwnedCountsMap
  } = {},
) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/cards/:game/cards/:id', component: { template: '<div />' } }],
  })
  const pinia = createPinia()
  setActivePinia(pinia)
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return mount(CollectionGrid, {
    props: {
      game: 'mtg',
      entries,
      list,
      collectionCounts: opts.collectionCounts,
      ownedMarks: opts.ownedMarks,
      wishlist: opts.wishlist,
    },
    global: { plugins: [router, pinia, [VueQueryPlugin, { queryClient }]] },
  })
}

// The count chips carry a semantic `aria-label` ("3 total" / "1 foil"). Count the "total"
// chips to know how many tiles show an owned-count badge without depending on styling.
function totalBadges(wrapper: ReturnType<typeof mountGrid>) {
  return wrapper.findAll('span').filter((s) => (s.attributes('aria-label') ?? '').endsWith('total'))
}

// The wish-list Heart chip carries an `aria-label` ending in "wanted"; count these to know
// how many tiles flag a wish-listed card without depending on styling.
function wantedBadges(wrapper: ReturnType<typeof mountGrid>) {
  return wrapper
    .findAll('span')
    .filter((s) => (s.attributes('aria-label') ?? '').endsWith('wanted'))
}

describe('CollectionGrid quick-add controls on the wishlist surface', () => {
  it('decouples the collection total from the wanted heart (collectionCounts overlay)', () => {
    // On the wishlist page each entry's own counts are WANTS; the always-collection-primary
    // control's total chips come from the `collectionCounts` overlay instead. Distinct 4-vs-3
    // values catch a branch swap in either primaryCounts (total) or wantedTotal (heart).
    const wrapper = mountGrid([entry('a', 2, 1), entry('b')], 'wishlist', {
      collectionCounts: { a: { quantity: 4, foil_quantity: 0 } },
    })
    // Card A: a collection total chip (4, from collectionCounts) + a wanted Heart (3, from the
    // entry's own 2 + 1 wants), and a trigger that edits the COLLECTION.
    expect(wrapper.find('[aria-label="4 total"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="3 wanted"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="Edit copies of Card a in your collection"]').exists()).toBe(
      true,
    )
    // Card B: no collection counts and no wants → an add-to-collection trigger.
    expect(wrapper.find('[aria-label="Add Card b to your collection"]').exists()).toBe(true)
    // No control targets the wish list — it's collection-primary everywhere.
    expect(wrapper.findAll('[aria-label$="to your wish list"]')).toHaveLength(0)
  })

  it('rests a wanted-but-unowned card as a heart with an add-to-collection trigger', () => {
    // No collectionCounts entry for the card → the primary chips sit at zero (the trigger rests
    // as "Add", not "Edit"), yet the entry's own wanted total still lights the Heart.
    const wrapper = mountGrid([entry('a', 2, 0)], 'wishlist')
    expect(wrapper.find('[aria-label="2 wanted"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="Add Card a to your collection"]').exists()).toBe(true)
    // Nothing owned in the collection, so no total chip renders — just the heart.
    expect(totalBadges(wrapper)).toHaveLength(0)
  })
})

describe('CollectionGrid quick-add controls on the collection surface (default list)', () => {
  it("feeds the primary chips from each entry's own counts", () => {
    // The plain collection grid: the entries already hold collection counts, so entry {2, 1}
    // rests as "3 total" + "1 foil" with an edit trigger — no collectionCounts overlay needed.
    // (Every entry is an owned holding, so each rests as an edit trigger — there's no "add"
    // affordance on this surface, unlike a catalog CardGrid.)
    const wrapper = mountGrid([entry('a', 2, 1), entry('b', 1, 0)])
    expect(wrapper.find('[aria-label="3 total"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="1 foil"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="Edit copies of Card a in your collection"]').exists()).toBe(
      true,
    )
    expect(wrapper.find('[aria-label="Edit copies of Card b in your collection"]').exists()).toBe(
      true,
    )
  })

  it('lights the wanted heart from the wishlist overlay, distinct from the collection total', () => {
    // On a collection grid the Heart reads the `wishlist` overlay (not the entry). Distinct
    // 3-total-vs-1-wanted values catch a branch swap that would source the heart from the entry
    // counts instead.
    const wrapper = mountGrid([entry('a', 2, 1)], 'collection', {
      wishlist: { a: { quantity: 1, foil_quantity: 0 } },
    })
    expect(wrapper.find('[aria-label="3 total"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="1 wanted"]').exists()).toBe(true)
  })

  it('shows no heart chip without a wishlist overlay', () => {
    const wrapper = mountGrid([entry('a', 1, 0)])
    expect(wantedBadges(wrapper)).toHaveLength(0)
  })
})
