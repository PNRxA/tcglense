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
    legalities: null,
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
    wishlistReady?: boolean
    readonly?: boolean
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
      wishlistReady: opts.wishlistReady,
      readonly: opts.readonly,
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
  it('sources both the collection total and the wanted heart from overlays, not the entry', () => {
    // On the wishlist page each entry's own counts are wants, but NEITHER quick-add chip reads
    // them: the always-collection-primary total comes from the `collectionCounts` overlay, and
    // the "N wanted" heart from the order-independent `wishlist` overlay (`['wishlist-counts',
    // …]`) so a want edit repaints the heart in place instead of resorting the recency-sorted
    // tiles (issue #364 follow-up). Entry counts (9/9) distinct from both overlays catch a
    // regression that sourced either chip from the entry.
    const wrapper = mountGrid([entry('a', 9, 9), entry('b')], 'wishlist', {
      collectionCounts: { a: { quantity: 4, foil_quantity: 0 } },
      wishlist: { a: { quantity: 2, foil_quantity: 1 } },
    })
    // Card A: a collection total chip (4, from collectionCounts) + a wanted Heart (3, from the
    // wishlist overlay's 2 + 1), and a trigger that edits the COLLECTION. The entry's own 9/9
    // must surface nowhere.
    expect(wrapper.find('[aria-label="4 total"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="3 wanted"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="18 wanted"]').exists()).toBe(false)
    expect(wrapper.find('[aria-label="Edit copies of Card a in your collection"]').exists()).toBe(
      true,
    )
    // Card B: no collection counts → an add-to-collection trigger. Absent from the wishlist
    // overlay, its heart falls back to the entry's own want (1) so a wanted tile never blanks
    // while the overlay is still loading (issue #364 follow-up F1). Two hearts total: card A from
    // the overlay (3), card B from the entry fallback (1).
    expect(wrapper.find('[aria-label="Add Card b to your collection"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="1 wanted"]').exists()).toBe(true)
    expect(wantedBadges(wrapper)).toHaveLength(2)
    // No control targets the wish list — it's collection-primary everywhere.
    expect(wrapper.findAll('[aria-label$="to your wish list"]')).toHaveLength(0)
  })

  it('rests a wanted-but-unowned card as a heart (from the wishlist overlay) with an add trigger', () => {
    // No collectionCounts entry → the primary chips sit at zero (the trigger rests as "Add",
    // not "Edit"), yet the wishlist overlay's want still lights the Heart. The entry's own
    // counts are irrelevant on this surface.
    const wrapper = mountGrid([entry('a', 0, 0)], 'wishlist', {
      wishlist: { a: { quantity: 2, foil_quantity: 0 } },
    })
    expect(wrapper.find('[aria-label="2 wanted"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="Add Card a to your collection"]').exists()).toBe(true)
    // Nothing owned in the collection, so no total chip renders — just the heart.
    expect(totalBadges(wrapper)).toHaveLength(0)
  })

  it("falls back to the entry's own wanted counts until the overlay lands", () => {
    // The `wishlist-counts` overlay is a sequential second fetch keyed off the just-painted list,
    // so on a cold load / every pagination it lands a round-trip after the tiles. Until it covers
    // a card, the wishlist surface falls back to the ENTRY's own wanted counts so the heart shows
    // immediately instead of blanking (issue #364 follow-up F1). Here a wishlist entry with 5
    // wants and NO overlay lights a "5 wanted" heart off the entry. Removing the per-card fallback
    // (heart reads only the overlay) drops this to 0 and the assertion fails.
    const wrapper = mountGrid([entry('a', 5, 0)], 'wishlist')
    expect(wrapper.find('[aria-label="5 wanted"]').exists()).toBe(true)
    expect(wantedBadges(wrapper)).toHaveLength(1)
  })

  it('prefers the overlay over the entry when both are present (overlay wins)', () => {
    // Once the overlay covers a card it takes over per-card, so an in-place want edit repaints the
    // heart from the overlay rather than the (stale, frozen) entry. A wishlist entry of 5 wants
    // with an overlay of 2 must read "2 wanted", not "5 wanted" — the overlay wins over the
    // fallback whenever it's present.
    const wrapper = mountGrid([entry('a', 5, 0)], 'wishlist', {
      wishlist: { a: { quantity: 2, foil_quantity: 0 } },
    })
    expect(wrapper.find('[aria-label="2 wanted"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="5 wanted"]').exists()).toBe(false)
  })

  it('clears the heart once the SETTLED overlay drops a removed want (no stale-entry fallback)', () => {
    // The reported bug: a quick-remove (want → 0) drops the card from the `['wishlist-counts', …]`
    // overlay, but the list refetch is deferred so the frozen entry still carries the old want.
    // With the overlay marked SETTLED (`wishlistReady`), an absent card is trusted as genuinely
    // unwanted, so no heart renders — the fix that stops the stale entry from pinning "5 wanted".
    const settled = mountGrid([entry('a', 5, 0)], 'wishlist', {
      wishlist: {},
      wishlistReady: true,
    })
    expect(wantedBadges(settled)).toHaveLength(0)

    // Contrast: the SAME empty overlay while it's still loading (`wishlistReady` false, the
    // default) keeps the entry fallback so a wanted tile never blanks on a cold load / pagination.
    const loading = mountGrid([entry('a', 5, 0)], 'wishlist', { wishlist: {} })
    expect(loading.find('[aria-label="5 wanted"]').exists()).toBe(true)
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

describe('CollectionGrid read-only (public collection browse, issues #361/#362)', () => {
  // CollectionGrid renders OwnedCountControl UNCONDITIONALLY (no auth gate), so the public
  // collection browse must pass `readonly` — otherwise a signed-in viewer of someone else's
  // page would get an editor seeded with the owner's counts that writes into their OWN
  // collection. Read-only renders a static owned badge (total + foil) instead.
  it('renders a static owned badge (total + foil) and never an editor', () => {
    const wrapper = mountGrid([entry('a', 2, 1)], 'collection', { readonly: true })
    expect(wrapper.find('[aria-label="3 total"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="1 foil"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="Edit copies of Card a in your collection"]').exists()).toBe(
      false,
    )
    expect(wrapper.findAll('[aria-label^="Add Card"]')).toHaveLength(0)
  })

  it('renders no badge on a zero-count entry', () => {
    const wrapper = mountGrid([entry('a', 0, 0)], 'collection', { readonly: true })
    expect(totalBadges(wrapper)).toHaveLength(0)
  })
})
