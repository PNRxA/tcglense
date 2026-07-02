import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Card, OwnedCountsMap } from '@/lib/api'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'
import CardGrid from '../CardGrid.vue'
import { useAuthStore } from '@/stores/auth'

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
) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/cards/:game/cards/:id', component: { template: '<div />' } }],
  })
  const pinia = createPinia()
  setActivePinia(pinia)
  if (authenticated) useAuthStore().accessToken = 'test-token'
  // CardTile renders a RouterLink, CardGrid reads the card-size preference from a Pinia
  // store, and the quick-add control uses vue-query, so the tree needs all three.
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return mount(CardGrid, {
    props: { game: 'mtg', cards, ownership, ghostUnowned, list },
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

  it('retargets the controls at the wish list when the grid is (issue #167)', () => {
    const wrapper = mountGrid(
      [makeCard('a'), makeCard('b')],
      { a: { quantity: 2, foil_quantity: 1 } },
      true,
      false,
      'wishlist',
    )
    // Owned card A keeps its count chips; unowned card B's add affordance targets the
    // wish list — the controls write there instead of the collection.
    expect(wrapper.find('[aria-label="3 total"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="Add Card b to your wish list"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="Add Card b to your collection"]').exists()).toBe(false)
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
