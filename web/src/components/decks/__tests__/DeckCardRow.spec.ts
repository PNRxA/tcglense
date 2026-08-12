import { describe, expect, it } from 'vitest'

import { mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import DeckCardRow from '../DeckCardRow.vue'
import OwnedCountBadge from '@/components/cards/OwnedCountBadge.vue'
import type { Card, DeckCardEntry } from '@/lib/api'

const card: Card = {
  id: 'zada',
  name: 'Zada, Hedron Grinder',
  set_code: 'ogw',
  set_name: 'Oath of the Gatewatch',
  collector_number: '111',
  rarity: 'rare',
  lang: 'en',
  released_at: '2016-01-22',
  mana_cost: '{3}{R}',
  cmc: 4,
  type_line: 'Legendary Creature — Goblin Ally',
  oracle_text: null,
  power: '3',
  toughness: '3',
  loyalty: null,
  color_identity: ['R'],
  colors: ['R'],
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

const entry: DeckCardEntry = { card, section_id: 1, quantity: 1, foil_quantity: 1 }

// The row links to the card page and reads the display currency (a Pinia store that observes
// the conversion rates through vue-query), so the tree needs all three plugins.
function mountRow(control = '') {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/cards/:game/cards/:id', component: { template: '<div />' } }],
  })
  const pinia = createPinia()
  setActivePinia(pinia)
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return mount(DeckCardRow, {
    props: { game: 'mtg', entry },
    slots: { control },
    global: {
      plugins: [router, pinia, [VueQueryPlugin, { queryClient }]],
      components: { OwnedCountBadge },
    },
  })
}

describe('DeckCardRow', () => {
  // The owner's control renders one chip per finish, so a card with foil copies carries two.
  // A fixed width there let those chips overflow the cell and paint over the card name: the
  // column reserves room for the pair, and reserves it as a floor rather than a cap.
  it('sizes the control column as a minimum, so a two-chip control cannot overflow it', () => {
    const wrapper = mountRow(
      '<OwnedCountBadge :quantity="1" :foil-quantity="1" kind="owned" :tooltip="false" />',
    )

    const badge = wrapper.findComponent(OwnedCountBadge)
    const cell = badge.element.parentElement as HTMLElement
    const classes = [...cell.classList]
    expect(classes).toContain('min-w-18')
    expect(classes.some((c) => /^(sm:)?w-\d/.test(c))).toBe(false)
    // One width for every breakpoint: a name may not start in a different place on a phone
    // depending on whether its row happens to carry a foil chip.
    expect(classes.some((c) => /^(sm|md|lg):min-w-/.test(c))).toBe(false)

    // Both chips live in that one cell — the width it reserves is for the pair, not one chip.
    const chips = badge
      .findAll('span')
      .filter((s) => /(total|foil)$/.test(s.attributes('aria-label') ?? ''))
    expect(chips.map((c) => c.attributes('aria-label'))).toEqual(['2 total', '1 foil'])
  })

  it('keeps the card name in its own column, linking to the card page', () => {
    const link = mountRow().find('a')
    expect(link.attributes('href')).toBe('/cards/mtg/cards/zada')
    expect(link.text()).toBe('Zada, Hedron Grinder')
  })
})
