import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Card } from '@/lib/api'
import CardMetaList from '../CardMetaList.vue'

// A complete card, overridden per test with just the Secret Lair fields that matter here.
function makeCard(overrides: Partial<Card> = {}): Card {
  return {
    id: 'x',
    name: 'Solitude',
    set_code: 'sld',
    set_name: 'Secret Lair',
    collector_number: '7004',
    rarity: 'mythic',
    lang: 'en',
    released_at: '2025-06-01',
    mana_cost: null,
    cmc: null,
    type_line: 'Creature — Elemental Incarnation',
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
    faces: [],
    ...overrides,
  }
}

function mountMeta(card: Card) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/cards/:game/sets/:code', component: { template: '<div />' } },
      { path: '/:pathMatch(.*)*', component: { template: '<div />' } },
    ],
  })
  return mount(CardMetaList, { props: { game: 'mtg', card }, global: { plugins: [router] } })
}

describe('CardMetaList Secret Lair relation', () => {
  it('links the drop to the by-drop view and marks a chase card (issue #295)', () => {
    const wrapper = mountMeta(
      makeCard({
        drop_name: 'FINAL FANTASY: Bonus Cards',
        drop_slug: 'final-fantasy-bonus-cards',
        secret_lair_bonus: true,
      }),
    )
    // The drop renders as a link into the set's by-drop view, filtered to this drop.
    const dropLink = wrapper.findAll('a').find((a) => a.text() === 'FINAL FANTASY: Bonus Cards')
    expect(dropLink, 'the drop should render as a link').toBeTruthy()
    expect(dropLink!.attributes('href')).toContain('/cards/mtg/sets/sld?drop=')
    // …and the card is marked as the chase card.
    expect(wrapper.text()).toContain('Chase card')
  })

  it('links the drop but shows no chase badge for a non-bonus drop card', () => {
    const wrapper = mountMeta(
      makeCard({
        drop_name: 'Cats of Chaos',
        drop_slug: 'cats-of-chaos',
        secret_lair_bonus: false,
      }),
    )
    const dropLink = wrapper.findAll('a').find((a) => a.text() === 'Cats of Chaos')
    expect(dropLink, 'the drop should still be a link').toBeTruthy()
    expect(wrapper.text()).not.toContain('Chase card')
  })

  it('shows no drop row for a card outside a drop-grouped set', () => {
    const wrapper = mountMeta(makeCard({ drop_name: null, drop_slug: null }))
    expect(wrapper.text()).not.toContain('Drop')
    expect(wrapper.text()).not.toContain('Chase card')
  })
})
