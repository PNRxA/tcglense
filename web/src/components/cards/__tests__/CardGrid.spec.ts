import { describe, it, expect } from 'vitest'

import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Card, OwnedCountsMap } from '@/lib/api'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'
import type { GhostStyle } from '@/lib/ghostDisplay'
import CardGrid from '../CardGrid.vue'
import { useAuthStore } from '@/stores/auth'
import { useGhostDisplayStore } from '@/stores/ghostDisplay'
import { useCardSizeStore } from '@/stores/cardSize'
import { CARD_SIZE_GRID_CLASS, PRODUCT_CARD_SIZE_GRID_CLASS, type CardSize } from '@/lib/cardSize'

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

// Signed in unless `authenticated: false`, since the quick-add controls (and thus the
// owned-count chips they carry) only render for a signed-in user.
function mountGrid(
  cards: Card[],
  ownership?: OwnedCountsMap,
  authenticated = true,
  ghostUnowned = false,
  list: CardListTarget = 'collection',
  opts: {
    ownedMarks?: OwnedCountsMap
    ghostStyle?: GhostStyle
    wishlist?: OwnedCountsMap
    collectionCounts?: OwnedCountsMap
    readonly?: boolean
    sizeClasses?: Record<CardSize, string>
    size?: CardSize
  } = {},
) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/cards/:game/cards/:id', component: { template: '<div />' } }],
  })
  const pinia = createPinia()
  setActivePinia(pinia)
  if (authenticated) useAuthStore().accessToken = 'test-token'
  // The ghost desaturation style (grayscale default / full colour) is a Pinia preference the
  // tile reads at setup, so set it before mounting (issue #213).
  if (opts.ghostStyle) useGhostDisplayStore().setStyle(opts.ghostStyle)
  // The grid maps the persisted size preference to column classes, so set it before mounting
  // when a test cares about which density row is chosen.
  if (opts.size) useCardSizeStore().setSize(opts.size)
  // CardTile renders a RouterLink, CardGrid reads the card-size preference from a Pinia
  // store, and the quick-add control uses vue-query, so the tree needs all three.
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return mount(CardGrid, {
    props: {
      game: 'mtg',
      cards,
      ownership,
      ghostUnowned,
      list,
      ownedMarks: opts.ownedMarks,
      wishlist: opts.wishlist,
      collectionCounts: opts.collectionCounts,
      readonly: opts.readonly,
      sizeClasses: opts.sizeClasses,
    },
    global: { plugins: [router, pinia, [VueQueryPlugin, { queryClient }]] },
  })
}

// The ghost treatment dims a card's text link with `opacity-60` (the desaturation lives on
// the image, off the link, so the stretched-link overlay keeps covering the whole tile).
function cardLink(wrapper: ReturnType<typeof mountGrid>, id: string) {
  return wrapper.find(`a[href="/cards/mtg/cards/${id}"]`)
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

describe('CardGrid detail modal links', () => {
  it('swaps an open sealed-product modal for the card modal, remembering the product', async () => {
    const wrapper = mountGrid([makeCard('a')], undefined, false)
    const router = wrapper.vm.$router
    await router.push('/cards/mtg/cards/a?product=product-1')

    await cardLink(wrapper, 'a').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.query.product).toBeUndefined()
    expect(router.currentRoute.value.query.card).toBe('a')
    // The product the card was opened from is stashed so the modal can offer "← Back to <product>".
    expect(router.currentRoute.value.query.openedFrom).toBe('product:product-1')
  })

  it('takes the product modal’s namespaced card search with it on the swap (#448)', async () => {
    // The swap closes the product modal, and `?pq=`/`?psort=` are that modal's per-product
    // state — leaving them behind would pre-filter the next product modal opened from here.
    const wrapper = mountGrid([makeCard('a')], undefined, false)
    const router = wrapper.vm.$router
    await router.push('/cards/mtg/cards/a?product=product-1&pq=t:goblin&psort=name:desc')

    await cardLink(wrapper, 'a').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ card: 'a', openedFrom: 'product:product-1' })
  })

  it('remembers the card it was opened from on a card->card hop (another printing)', async () => {
    // Clicking another printing from inside a card modal ("Other printings") now remembers the
    // card you were on so the modal can offer "← Back to <card>" (issue #485) — the same one-tap
    // return a card<->product swap gives. Any stale cross-surface marker is replaced.
    const wrapper = mountGrid([makeCard('a')], undefined, false)
    const router = wrapper.vm.$router
    await router.push('/cards/mtg/cards/a?card=old&openedFrom=product:product-9')

    await cardLink(wrapper, 'a').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ card: 'a', openedFrom: 'card:old' })
  })

  it('clears a stale origin marker when opening from a plain browse grid', async () => {
    // A plain browse grid has no modal open (no `?card=`), so opening a card there is a fresh
    // start: any leftover `?openedFrom=` from an earlier trip must go, not point the card modal
    // back at an unrelated item.
    const wrapper = mountGrid([makeCard('a')], undefined, false)
    const router = wrapper.vm.$router
    await router.push('/cards/mtg/cards/a?openedFrom=product:product-9')

    await cardLink(wrapper, 'a').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ card: 'a' })
  })

  it('leaves the set-grouping ?from= set code untouched (distinct key)', async () => {
    // The related-sets grouped view carries `?from=<setCode>` on these same card pages; the modal
    // uses `?openedFrom=` so opening a card there must not clobber the grouping's `from`.
    const wrapper = mountGrid([makeCard('a')], undefined, false)
    const router = wrapper.vm.$router
    await router.push('/cards/mtg/cards/a?related=1&from=blc')

    await cardLink(wrapper, 'a').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ related: '1', from: 'blc', card: 'a' })
  })
})

describe('CardGrid quick-add controls', () => {
  it('shows a total (+ foil) count on owned cards and an add affordance on the rest', () => {
    const wrapper = mountGrid([makeCard('a'), makeCard('b')], {
      a: { quantity: 2, foil_quantity: 1 },
    })
    // Owned card A: total is regular + foil (3), with a separate foil chip (1).
    expect(wrapper.find('[aria-label="3 total"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="1 foil"]').exists()).toBe(true)
    // Exactly one tile shows an owned-count badge (card A).
    expect(totalBadges(wrapper)).toHaveLength(1)
    // Unowned card B instead offers an "add to collection" trigger.
    expect(wrapper.find('[aria-label="Add Card b to your collection"]').exists()).toBe(true)
  })

  it('shows no foil chip for a card owned only in regular', () => {
    const wrapper = mountGrid([makeCard('a')], { a: { quantity: 3, foil_quantity: 0 } })
    expect(wrapper.find('[aria-label="3 total"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="0 foil"]').exists()).toBe(false)
  })

  it('offers add triggers but no count badges when nothing is owned', () => {
    const wrapper = mountGrid([makeCard('a'), makeCard('b')])
    expect(totalBadges(wrapper)).toHaveLength(0)
    expect(wrapper.findAll('[aria-label^="Add Card"]')).toHaveLength(2)
  })

  it('renders no controls at all for a signed-out visitor', () => {
    const wrapper = mountGrid(
      [makeCard('a'), makeCard('b')],
      { a: { quantity: 2, foil_quantity: 1 } },
      false,
    )
    expect(totalBadges(wrapper)).toHaveLength(0)
    expect(wrapper.findAll('[aria-label^="Add Card"]')).toHaveLength(0)
    // The tiles themselves still render as links to each card page.
    expect(wrapper.find('a[href="/cards/mtg/cards/a"]').exists()).toBe(true)
  })

  it('mounts a collection-primary control on the wishlist surface (collectionCounts overlay)', () => {
    // On the wishlist page the grid's own `ownership` map is WANTS; the collection totals for
    // the always-collection-primary control come from the `collectionCounts` overlay instead.
    const wrapper = mountGrid(
      [makeCard('a'), makeCard('b')],
      { a: { quantity: 2, foil_quantity: 1 } },
      true,
      false,
      'wishlist',
      { collectionCounts: { a: { quantity: 4, foil_quantity: 0 } } },
    )
    // Card A: a collection total chip (4, from collectionCounts) + a wanted Heart (3, from the
    // grid's own ownership), and an owned trigger that edits the COLLECTION.
    expect(wrapper.find('[aria-label="4 total"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="3 wanted"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="Edit copies of Card a in your collection"]').exists()).toBe(
      true,
    )
    // Card B: neither owned nor wanted → an add-to-collection trigger.
    expect(wrapper.find('[aria-label="Add Card b to your collection"]').exists()).toBe(true)
    // No control targets the wish list anymore — it's collection-primary everywhere.
    expect(wrapper.findAll('[aria-label$="to your wish list"]')).toHaveLength(0)
  })
})

describe('CardGrid wish-list hearts (issue #364)', () => {
  it('shows both a total and a wanted chip on an owned + wish-listed card', () => {
    const wrapper = mountGrid(
      [makeCard('a'), makeCard('b')],
      { a: { quantity: 2, foil_quantity: 1 } },
      true,
      false,
      'collection',
      { wishlist: { a: { quantity: 1, foil_quantity: 0 } } },
    )
    expect(wrapper.find('[aria-label="3 total"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="1 wanted"]').exists()).toBe(true)
  })

  it('rests a wish-listed-but-unowned card as a heart with an add-to-collection trigger', () => {
    const wrapper = mountGrid([makeCard('a')], undefined, true, false, 'collection', {
      wishlist: { a: { quantity: 2, foil_quantity: 0 } },
    })
    expect(wrapper.find('[aria-label="2 wanted"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="Add Card a to your collection"]').exists()).toBe(true)
    // Not owned, so no total chip renders — just the heart.
    expect(totalBadges(wrapper)).toHaveLength(0)
  })

  it('shows no heart chip without a wishlist map', () => {
    const wrapper = mountGrid([makeCard('a')], { a: { quantity: 1, foil_quantity: 0 } })
    expect(wantedBadges(wrapper)).toHaveLength(0)
  })
})

describe('CardGrid show-ghosts mode (issue #112)', () => {
  it('dims only the cards the viewer does not own', () => {
    const wrapper = mountGrid(
      [makeCard('a'), makeCard('b')],
      { a: { quantity: 1, foil_quantity: 0 } },
      true,
      true,
    )
    // Owned card A renders at full strength; unowned card B is ghosted (dimmed).
    expect(cardLink(wrapper, 'a').classes()).not.toContain('opacity-60')
    expect(cardLink(wrapper, 'b').classes()).toContain('opacity-60')
  })

  it('treats a zero-count ownership entry as unowned', () => {
    const wrapper = mountGrid([makeCard('a')], { a: { quantity: 0, foil_quantity: 0 } }, true, true)
    expect(cardLink(wrapper, 'a').classes()).toContain('opacity-60')
  })

  it('dims nothing when ghost mode is off, even for unowned cards', () => {
    const wrapper = mountGrid([makeCard('a'), makeCard('b')], {
      a: { quantity: 1, foil_quantity: 0 },
    })
    expect(cardLink(wrapper, 'a').classes()).not.toContain('opacity-60')
    expect(cardLink(wrapper, 'b').classes()).not.toContain('opacity-60')
  })

  it('keeps a ghosted card fully clickable (grayscale is on the image, not the link)', () => {
    const wrapper = mountGrid([makeCard('b')], {}, true, true)
    // The stretched-link overlay must stay on the link: a `filter` there would collapse it.
    // Guard that the link itself never carries grayscale (it lives on the image instead).
    expect(cardLink(wrapper, 'b').classes()).not.toContain('grayscale')
  })
})

describe('CardGrid ghost desaturation style (issue #213)', () => {
  // The desaturation lives on the CardImage root; both modes dim it (opacity-45), and only
  // grayscale mode drains the colour.
  it('drains a ghost image of colour by default (grayscale mode)', () => {
    const wrapper = mountGrid([makeCard('b')], {}, true, true)
    expect(wrapper.find('.grayscale').exists()).toBe(true)
    expect(wrapper.find('.opacity-45').exists()).toBe(true)
  })

  it('keeps a ghost image in colour (dim only) in colour mode', () => {
    const wrapper = mountGrid([makeCard('b')], {}, true, true, 'collection', {
      ghostStyle: 'color',
    })
    expect(wrapper.find('.grayscale').exists()).toBe(false)
    // Still dimmed, so owned cards keep standing out.
    expect(wrapper.find('.opacity-45').exists()).toBe(true)
  })

  it('never desaturates or dims when ghost mode is off', () => {
    const wrapper = mountGrid([makeCard('b')], {}, true, false)
    expect(wrapper.find('.grayscale').exists()).toBe(false)
    expect(wrapper.find('.opacity-45').exists()).toBe(false)
  })
})

describe('CardGrid read-only (public collection browse, issues #361/#362)', () => {
  // On a public collection's show-ghosts grid the `ownership` map is the OWNER's counts. The
  // `readonly` flag must render a static owned badge and NEVER a quick-add editor — even for a
  // signed-in viewer, whose editor would otherwise write the owner's counts into their OWN
  // collection.
  it('renders static owned badges and never an editor, even for a signed-in viewer', () => {
    const wrapper = mountGrid(
      [makeCard('a'), makeCard('b')],
      { a: { quantity: 2, foil_quantity: 1 } },
      true, // signed in
      true, // show-ghosts
      'collection',
      { readonly: true },
    )
    // The owner's owned card A shows a static badge (3 total, 1 foil)...
    expect(wrapper.find('[aria-label="3 total"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="1 foil"]').exists()).toBe(true)
    expect(totalBadges(wrapper)).toHaveLength(1)
    // ...with NO editor and NO add trigger for the viewer.
    expect(wrapper.findAll('[aria-label^="Edit copies"]')).toHaveLength(0)
    expect(wrapper.findAll('[aria-label^="Add Card"]')).toHaveLength(0)
    // Unowned card B is still ghosted (dimmed) and carries no badge.
    expect(cardLink(wrapper, 'b').classes()).toContain('opacity-60')
  })
})

describe('CardGrid owned-in-collection marks (issue #213)', () => {
  // On the wish-list browse grids, "Show owned (in collection)" overlays an "Owned" marker on
  // cards the viewer owns in their collection (passed in as `ownedMarks`).
  it('marks a card present in ownedMarks with a positive count', () => {
    const wrapper = mountGrid([makeCard('a'), makeCard('b')], undefined, true, false, 'wishlist', {
      ownedMarks: { a: { quantity: 1, foil_quantity: 0 } },
    })
    expect(wrapper.findAll('[aria-label="Owned in your collection"]')).toHaveLength(1)
  })

  it('treats a zero-count owned mark as not owned', () => {
    const wrapper = mountGrid([makeCard('a')], undefined, true, false, 'wishlist', {
      ownedMarks: { a: { quantity: 0, foil_quantity: 0 } },
    })
    expect(wrapper.find('[aria-label="Owned in your collection"]').exists()).toBe(false)
  })

  it('shows no marks without an ownedMarks map', () => {
    const wrapper = mountGrid([makeCard('a')], undefined, true, false, 'wishlist')
    expect(wrapper.find('[aria-label="Owned in your collection"]').exists()).toBe(false)
  })
})

describe('CardGrid density map', () => {
  // The grid root carries the column classes for the persisted size. By default that's the
  // catalog scale; a caller (the sealed-product "Cards in this product" section) can pass a
  // bumped-up scale that renders each selection one size larger.
  it('uses the catalog scale by default', () => {
    const wrapper = mountGrid([makeCard('a')], undefined, false, false, 'collection', {
      size: 'large',
    })
    expect(wrapper.classes()).toContain('xl:grid-cols-4') // CARD_SIZE_GRID_CLASS.large
    expect(CARD_SIZE_GRID_CLASS.large).toContain('xl:grid-cols-4')
  })

  it('applies a provided sizeClasses override for the same size preference', () => {
    // Product scale's `large` goes bigger than the catalog's — fewer columns at xl.
    const wrapper = mountGrid([makeCard('a')], undefined, false, false, 'collection', {
      size: 'large',
      sizeClasses: PRODUCT_CARD_SIZE_GRID_CLASS,
    })
    expect(wrapper.classes()).toContain('xl:grid-cols-3') // PRODUCT_CARD_SIZE_GRID_CLASS.large
    expect(wrapper.classes()).not.toContain('xl:grid-cols-4')
  })

  it('shifts each product-scale selection one size up from the catalog scale', () => {
    // The product page's `small` matches the catalog's `medium`, and its `medium` matches the
    // catalog's `large` — the promised one-step-larger shift, per selection.
    expect(PRODUCT_CARD_SIZE_GRID_CLASS.small).toBe(CARD_SIZE_GRID_CLASS.medium)
    expect(PRODUCT_CARD_SIZE_GRID_CLASS.medium).toBe(CARD_SIZE_GRID_CLASS.large)
  })
})
