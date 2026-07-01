import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Card, OwnedCountsMap } from '@/lib/api'
import CardGrid from '../cards/CardGrid.vue'

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

function mountGrid(cards: Card[], ownership?: OwnedCountsMap) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/cards/:game/cards/:id', component: { template: '<div />' } }],
  })
  // CardTile renders a RouterLink, and CardGrid reads the card-size preference from a
  // Pinia store, so the mounted tree needs both.
  return mount(CardGrid, {
    props: { game: 'mtg', cards, ownership },
    global: { plugins: [router, createPinia()] },
  })
}

// The badge chips carry a semantic `title` ("3 total" / "1 foil"); count the "total"
// chips to know how many tiles got a badge without depending on styling classes.
function totalBadges(wrapper: ReturnType<typeof mountGrid>) {
  return wrapper.findAll('span').filter((s) => (s.attributes('title') ?? '').endsWith('total'))
}

describe('CardGrid ownership badges', () => {
  it('overlays a total (+ foil) badge only on owned cards', () => {
    const wrapper = mountGrid([makeCard('a'), makeCard('b')], {
      a: { quantity: 2, foil_quantity: 1 },
    })
    // Owned card A: total is regular + foil (3), with a separate foil chip (1).
    expect(wrapper.find('[title="3 total"]').exists()).toBe(true)
    expect(wrapper.find('[title="1 foil"]').exists()).toBe(true)
    // Unowned card B gets none, so exactly one tile carries a badge.
    expect(totalBadges(wrapper)).toHaveLength(1)
  })

  it('shows no foil chip for a card owned only in regular', () => {
    const wrapper = mountGrid([makeCard('a')], { a: { quantity: 3, foil_quantity: 0 } })
    expect(wrapper.find('[title="3 total"]').exists()).toBe(true)
    expect(wrapper.find('[title="0 foil"]').exists()).toBe(false)
  })

  it('renders no badges when no ownership map is provided', () => {
    const wrapper = mountGrid([makeCard('a'), makeCard('b')])
    expect(totalBadges(wrapper)).toHaveLength(0)
  })
})
