import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Card, CardDetail, CardDetailOrTile } from '@/lib/api'
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
    drop_noun: null,
    secret_lair_bonus: false,
    secret_lair_spend_incentive: false,
    faces: [],
    legalities: null,
    ...overrides,
  }
}

// The same card once the single-card route has answered: every detail-only field present
// (issue #673). A plain `makeCard` stands for the other half of the union — the grid tile
// the query is seeded from, which carries none of them.
function makeDetail(overrides: Partial<CardDetail> = {}): CardDetail {
  return {
    ...makeCard(),
    artist: null,
    artist_ids: [],
    illustration_id: null,
    flavor_text: null,
    watermark: null,
    finishes: [],
    frame: null,
    frame_effects: [],
    border_color: null,
    security_stamp: null,
    promo_types: [],
    produced_mana: [],
    defense: null,
    reserved: false,
    full_art: false,
    textless: false,
    promo: false,
    variation: false,
    story_spotlight: false,
    content_warning: false,
    edhrec_rank: null,
    penny_rank: null,
    ...overrides,
  }
}

function mountMeta(card: CardDetailOrTile) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/cards/:game/sets/:code', component: { template: '<div />' } },
      { path: '/cards/:game/cards', component: { template: '<div />' } },
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
        drop_noun: null,
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

  it('heads the row with the group noun, so a Zeta Set treatment is not called a drop', () => {
    const treatment = mountMeta(
      makeCard({
        set_code: 'slz',
        set_name: 'The Zeta Set',
        drop_name: 'Photocopy Negatives',
        drop_slug: 'photocopy-negatives',
        drop_noun: 'treatment',
      }),
    )
    expect(treatment.text()).toContain('Treatment')
    expect(treatment.text()).not.toContain('Drop')
    const link = treatment.findAll('a').find((a) => a.text() === 'Photocopy Negatives')
    expect(link, 'the treatment section is still linked').toBeTruthy()

    // A Secret Lair drop keeps its heading; a card the API predates the noun on reads "Drop".
    const drop = mountMeta(
      makeCard({ drop_name: 'Cats of Chaos', drop_slug: 'cats-of-chaos', drop_noun: 'drop' }),
    )
    expect(drop.text()).toContain('Drop')
    const legacy = mountMeta(
      makeCard({ drop_name: 'Cats of Chaos', drop_slug: 'cats-of-chaos', drop_noun: null }),
    )
    expect(legacy.text()).toContain('Drop')
  })

  it('links the drop but shows no chase badge for a non-bonus drop card', () => {
    const wrapper = mountMeta(
      makeCard({
        drop_name: 'Cats of Chaos',
        drop_slug: 'cats-of-chaos',
        drop_noun: null,
        secret_lair_bonus: false,
      }),
    )
    const dropLink = wrapper.findAll('a').find((a) => a.text() === 'Cats of Chaos')
    expect(dropLink, 'the drop should still be a link').toBeTruthy()
    expect(wrapper.text()).not.toContain('Chase card')
  })

  it('marks a spend-reward promo distinctly, in place of the chase badge (issue #331)', () => {
    const wrapper = mountMeta(
      makeCard({
        drop_name: 'Promos / Special',
        drop_slug: 'promos-special',
        drop_noun: null,
        // A spend incentive is tagged sldbonus too, but the spend badge takes precedence.
        secret_lair_bonus: true,
        secret_lair_spend_incentive: true,
      }),
    )
    expect(wrapper.text()).toContain('Spend reward')
    expect(wrapper.text()).not.toContain('Chase card')
  })

  it('shows the spend-reward badge even when the promo is not grouped into a drop', () => {
    // e.g. Arcane Signet #908 — a spend reward the drop snapshot does not group.
    const wrapper = mountMeta(
      makeCard({ drop_name: null, drop_slug: null, secret_lair_spend_incentive: true }),
    )
    expect(wrapper.text()).toContain('Spend reward')
    // No drop row, since it has no drop.
    expect(wrapper.text()).not.toContain('Drop')
  })

  it('shows no drop row for a card outside a drop-grouped set', () => {
    const wrapper = mountMeta(makeCard({ drop_name: null, drop_slug: null }))
    expect(wrapper.text()).not.toContain('Drop')
    expect(wrapper.text()).not.toContain('Chase card')
  })
})

describe('CardMetaList print details (issue #673)', () => {
  it('links the artist to the card search for that credit', () => {
    const wrapper = mountMeta(makeDetail({ artist: 'Rebecca Guay' }))
    const link = wrapper.findAll('a').find((a) => a.text() === 'Rebecca Guay')
    expect(link, 'the artist should render as a link').toBeTruthy()
    const href = link!.attributes('href') ?? ''
    expect(href).toContain('/cards/mtg/cards?q=')
    // A name with spaces is quoted by the shared search builder: a:"Rebecca Guay".
    const q = new URLSearchParams(href.slice(href.indexOf('?'))).get('q')
    expect(q).toBe('a:"Rebecca Guay"')
  })

  it('shows a Battle\u2019s defense even though it is a two-faced card', () => {
    const battle = makeDetail({
      name: 'Invasion of Alara',
      type_line: 'Battle \u2014 Siege',
      defense: '5',
      power: null,
      toughness: null,
      faces: [
        {
          name: 'Invasion of Alara',
          mana_cost: '{W}{U}{B}{R}{G}',
          type_line: 'Battle \u2014 Siege',
          oracle_text: null,
          power: null,
          toughness: null,
          loyalty: null,
        },
        {
          name: 'Awaken the Maelstrom',
          mana_cost: null,
          type_line: 'Sorcery',
          oracle_text: null,
          power: null,
          toughness: null,
          loyalty: null,
        },
      ],
    })
    const wrapper = mountMeta(battle)
    expect(wrapper.text()).toContain('Defense')
    expect(wrapper.text()).toContain('5')
    // P/T stays per-face for a multi-faced card, so its row is absent either way.
    expect(wrapper.text()).not.toContain('Power / Toughness')
  })

  it('names each finish the printing comes in', () => {
    const wrapper = mountMeta(makeDetail({ finishes: ['nonfoil', 'foil', 'etched'] }))
    expect(wrapper.text()).toContain('Finishes')
    expect(wrapper.text()).toContain('Regular')
    expect(wrapper.text()).toContain('Foil')
    expect(wrapper.text()).toContain('Etched foil')
  })

  it('humanises the frame, promo types and printing flags', () => {
    const wrapper = mountMeta(
      makeDetail({
        frame: '2015',
        frame_effects: ['showcase', 'extendedart'],
        border_color: 'borderless',
        security_stamp: 'oval',
        watermark: 'boros',
        promo_types: ['buyabox', 'surgefoil'],
        full_art: true,
        content_warning: true,
      }),
    )
    expect(wrapper.text()).toContain('2015 \u00b7 Showcase, Extended art')
    expect(wrapper.text()).toContain('Borderless')
    expect(wrapper.text()).toContain('Oval')
    expect(wrapper.text()).toContain('Boros')
    expect(wrapper.text()).toContain('Buy-a-Box')
    expect(wrapper.text()).toContain('Surge foil')
    expect(wrapper.text()).toContain('Full art')
    expect(wrapper.text()).toContain('Content warning')
    // Only the true flags are chipped.
    expect(wrapper.text()).not.toContain('Textless')
  })

  it('marks the Reserved List only for a card on it', () => {
    expect(mountMeta(makeDetail({ reserved: true })).text()).toContain(
      'Reserved List \u2014 never to be reprinted',
    )
    expect(mountMeta(makeDetail({ reserved: false })).text()).not.toContain('Reserved List')
  })

  it('formats the popularity ranks and omits an unranked one', () => {
    const wrapper = mountMeta(makeDetail({ edhrec_rank: 12345, penny_rank: null }))
    expect(wrapper.text()).toContain('EDHREC rank')
    expect(wrapper.text()).toContain(`#${(12345).toLocaleString()}`)
    expect(wrapper.text()).not.toContain('Penny rank')
  })

  it('renders none of the detail rows for a plain grid tile, and does not throw', () => {
    // The placeholder case: the query seeded from a card grid, which carries no detail
    // fields at all — every one of those rows must simply be absent.
    const wrapper = mountMeta(makeCard({ rarity: 'rare' }))
    for (const row of [
      'Artist',
      'Produces',
      'Defense',
      'Finishes',
      'Frame',
      'Border',
      'Stamp',
      'Watermark',
      'Promo types',
      'Printing',
      'Reserved List',
      'EDHREC rank',
      'Penny rank',
    ]) {
      expect(wrapper.text(), `${row} should not render for a grid tile`).not.toContain(row)
    }
    // …while the shared Card half still renders.
    expect(wrapper.text()).toContain('Secret Lair')
    expect(wrapper.text()).toContain('rare')
  })
})
