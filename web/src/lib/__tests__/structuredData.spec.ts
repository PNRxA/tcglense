import { describe, expect, it } from 'vitest'
import type { Card, Product, ProductComponent } from '@/lib/api'
import {
  assembleMetaDescription,
  breadcrumbList,
  capitalize,
  cardCrumbs,
  cardMetaDescription,
  cardProductNode,
  contentsSummary,
  graph,
  productMetaDescription,
  sealedCrumbs,
  sealedProductNode,
} from '../structuredData'

const ORIGIN = window.location.origin
const TAIL = 'Track its price history on TCGLense.'

// Neutral fixtures — a spec overrides only the fields it asserts on (see test/fixtures.ts idiom).
function makeCard(over: Partial<Card> = {}): Card {
  return {
    id: 'card-abc',
    name: "Assassin's Trophy",
    set_code: 'grn',
    set_name: 'Guilds of Ravnica',
    collector_number: '152',
    rarity: 'rare',
    lang: 'en',
    released_at: '2018-10-05',
    mana_cost: '{B}{G}',
    cmc: 2,
    type_line: 'Instant',
    oracle_text: 'Destroy target permanent an opponent controls.',
    power: null,
    toughness: null,
    loyalty: null,
    color_identity: ['B', 'G'],
    colors: ['B', 'G'],
    layout: 'normal',
    prices: { usd: '1.20', usd_foil: '3.50', eur: '1.00', tix: '0.50' },
    has_image: true,
    drop_name: null,
    drop_slug: null,
    secret_lair_bonus: false,
    secret_lair_spend_incentive: false,
    faces: [],
    legalities: null,
    ...over,
  }
}

function makeProduct(over: Partial<Product> = {}): Product {
  return {
    id: 'prod-1',
    name: 'Kaldheim Collector Booster Box',
    set_code: 'khm',
    set_name: 'Kaldheim',
    product_type: 'collector_display',
    url: null,
    has_image: true,
    prices: { usd: '199.99', usd_foil: null },
    msrp: null,
    released_at: '2021-02-05',
    ...over,
  }
}

function makeComponent(over: Partial<ProductComponent> = {}): ProductComponent {
  return {
    kind: 'sealed',
    name: 'Collector Booster',
    quantity: 12,
    product: null,
    card: null,
    ...over,
  }
}

describe('capitalize', () => {
  it('capitalizes the first letter and passes null/empty through', () => {
    expect(capitalize('rare')).toBe('Rare')
    expect(capitalize('')).toBe('')
    expect(capitalize(null)).toBe('')
    expect(capitalize(undefined)).toBe('')
  })
})

describe('assembleMetaDescription', () => {
  it('includes every clause under budget and always ends with the tail', () => {
    const out = assembleMetaDescription('Lead.', ['One.', 'Two.'])
    expect(out).toBe(`Lead. One. Two. ${TAIL}`)
    expect(out.endsWith(TAIL)).toBe(true)
  })

  it('drops the lowest-priority clause first when over budget, keeping the tail', () => {
    const out = assembleMetaDescription(
      'Lead.',
      ['High priority clause.', 'Low priority clause.'],
      'Tail.',
      40,
    )
    expect(out).toContain('High priority')
    expect(out).not.toContain('Low priority')
    expect(out.endsWith('Tail.')).toBe(true)
  })

  it('stops at the first clause too long to fit — a lower-priority clause cannot leapfrog it', () => {
    // The high-priority clause overflows on its own; the short low-priority one WOULD fit, but
    // strict priority means it is dropped too (e.g. sealed contents never displaced by the price).
    const longHigh =
      'A high-priority clause that is definitely far too long to ever fit the budget.'
    const out = assembleMetaDescription('Lead.', [longHigh, 'Low.'], 'Tail.', 40)
    expect(out).toBe('Lead. Tail.')
  })

  it('skips a nullish clause without stopping, so a later present clause is still included', () => {
    const out = assembleMetaDescription('Lead.', [null, 'Kept.'], 'Tail.', 40)
    expect(out).toBe('Lead. Kept. Tail.')
  })

  it('skips null/undefined/empty clauses and never emits null/undefined/NaN', () => {
    const out = assembleMetaDescription('Lead.', [null, undefined, '', 'Real.'])
    expect(out).toBe(`Lead. Real. ${TAIL}`)
    expect(out).not.toMatch(/null|undefined|NaN/)
  })

  it('still returns lead + tail when the lead alone exceeds the budget', () => {
    const lead = 'A'.repeat(200)
    const out = assembleMetaDescription(lead, ['x'], 'Tail.', 160)
    expect(out).toBe(`${lead} Tail.`)
    expect(out).not.toContain(' x ')
  })
})

describe('cardMetaDescription', () => {
  it('renders name — rarity type · set · #number, then the latest price', () => {
    const out = cardMetaDescription(makeCard())
    expect(out).toBe(
      "Assassin's Trophy — Rare Instant · Guilds of Ravnica · #152. Latest price $1.20. " + TAIL,
    )
    expect(out.length).toBeLessThanOrEqual(160)
  })

  it('collapses the descriptor to just the type when rarity is null', () => {
    const out = cardMetaDescription(makeCard({ rarity: null }))
    expect(out).toContain('— Instant · Guilds of Ravnica · #152.')
  })

  it('collapses the descriptor to just the rarity when the type is null', () => {
    const out = cardMetaDescription(makeCard({ type_line: null }))
    expect(out).toContain('— Rare · Guilds of Ravnica · #152.')
  })

  it('omits the descriptor and the price when both type/rarity and price are absent', () => {
    const out = cardMetaDescription(
      makeCard({
        rarity: null,
        type_line: null,
        prices: { usd: null, usd_foil: null, eur: null, tix: null },
      }),
    )
    expect(out).toBe("Assassin's Trophy — Guilds of Ravnica · #152. " + TAIL)
  })

  it('omits the price clause when there is no USD price', () => {
    const out = cardMetaDescription(
      makeCard({ prices: { usd: null, usd_foil: null, eur: null, tix: null } }),
    )
    expect(out).not.toContain('Latest price')
  })

  it('stays within 160 chars for a long card, dropping the price first', () => {
    const out = cardMetaDescription(
      makeCard({
        name: 'Urza, Lord High Artificer',
        rarity: 'mythic',
        type_line: 'Legendary Creature — Human Artificer Planeswalker',
        set_name: 'Modern Horizons',
        collector_number: '200',
        prices: { usd: '25.00', usd_foil: null, eur: null, tix: null },
      }),
    )
    expect(out.length).toBeLessThanOrEqual(160)
    expect(out).not.toContain('Latest price')
    expect(out).toContain('Urza, Lord High Artificer')
  })
})

describe('contentsSummary', () => {
  it('returns null for no valid components', () => {
    expect(contentsSummary([])).toBeNull()
    expect(contentsSummary([makeComponent({ quantity: 0 })])).toBeNull()
    expect(contentsSummary([makeComponent({ name: '' })])).toBeNull()
  })

  it('lists a single component with its quantity and no "and more"', () => {
    expect(contentsSummary([makeComponent({ name: 'Draft Booster', quantity: 36 })])).toBe(
      'Contains 36× Draft Booster.',
    )
  })

  it('lists exactly two components without "and more"', () => {
    const out = contentsSummary([
      makeComponent({ name: 'Set Booster', quantity: 12 }),
      makeComponent({ name: 'Promo Card', quantity: 1 }),
    ])
    expect(out).toBe('Contains 12× Set Booster, 1× Promo Card.')
  })

  it('appends "and more" past two components, and never pluralizes the name', () => {
    const out = contentsSummary([
      makeComponent({ name: 'Booster Box', quantity: 1 }),
      makeComponent({ name: 'Dice', quantity: 6 }),
      makeComponent({ name: 'Life Pad', quantity: 1 }),
    ])
    expect(out).toBe('Contains 1× Booster Box, 6× Dice and more.')
    expect(out).not.toContain('Boxs')
  })
})

describe('productMetaDescription', () => {
  const contents = [makeComponent({ name: 'Collector Booster', quantity: 12 })]

  it('does not repeat type/set context the product name already carries (anti-stuffing)', () => {
    const out = productMetaDescription(makeProduct(), 'Collector Booster Box', 'Kaldheim', contents)
    expect(out).toBe(
      'Kaldheim Collector Booster Box. Contains 12× Collector Booster. Latest price $199.99. ' +
        TAIL,
    )
    expect(out).not.toContain('— Collector Booster Box')
  })

  it('adds the type/set context when the name lacks it', () => {
    const out = productMetaDescription(
      makeProduct({ name: 'Draft Booster', prices: { usd: '4.50', usd_foil: null } }),
      'Draft Booster Pack',
      'Dominaria United',
      [],
    )
    expect(out).toContain('Draft Booster — Draft Booster Pack · Dominaria United.')
  })

  it('keeps the contents summary over the price when only one fits (issue #302 priority)', () => {
    const out = productMetaDescription(
      makeProduct({ name: 'Bundle Gift Edition', prices: { usd: '49.99', usd_foil: null } }),
      'Bundle',
      'March of the Machine',
      [
        makeComponent({ name: 'Play Booster Pack', quantity: 8 }),
        makeComponent({ name: 'Collector Booster Pack', quantity: 1 }),
        makeComponent({ name: 'Spindown die', quantity: 1 }),
      ],
    )
    expect(out).toContain('Contains 8× Play Booster Pack')
    expect(out).toContain('and more')
    expect(out).not.toContain('Latest price')
    expect(out.length).toBeLessThanOrEqual(160)
  })

  it('places the contents summary before the price and drops both label prose when absent', () => {
    const withContents = productMetaDescription(
      makeProduct(),
      'Collector Booster Box',
      'Kaldheim',
      contents,
    )
    expect(withContents.indexOf('Contains')).toBeLessThan(withContents.indexOf('Latest price'))

    const withoutContents = productMetaDescription(
      makeProduct({ prices: { usd: null, usd_foil: null } }),
      'Collector Booster Box',
      'Kaldheim',
      [],
    )
    expect(withoutContents).not.toContain('Contains')
    expect(withoutContents).not.toContain('Latest price')
  })
})

describe('cardProductNode', () => {
  it('builds a valid Product with the card facts and no purchasability claim', () => {
    const node = cardProductNode(makeCard(), 'https://cdn.example.com/large.jpg')
    expect(node['@type']).toBe('Product')
    expect(node.name).toBe("Assassin's Trophy")
    expect(node.brand).toEqual({ '@type': 'Brand', name: 'Guilds of Ravnica' })
    expect(node.category).toBe('Instant')
    expect(node.sku).toBe('GRN-152')
    expect(node.image).toBe('https://cdn.example.com/large.jpg')
    expect(node.releaseDate).toBe('2018-10-05')
  })

  it('omits the image when none is passed and drops a non-ISO release date', () => {
    const node = cardProductNode(makeCard({ released_at: 'sometime in 2018' }))
    expect('image' in node).toBe(false)
    expect('releaseDate' in node).toBe(false)
  })

  it('emits additionalProperty PropertyValues, omitting absent ones', () => {
    const props = cardProductNode(makeCard()).additionalProperty as {
      name: string
      value: unknown
    }[]
    const byName = Object.fromEntries(props.map((p) => [p.name, p.value]))
    expect(byName['Rarity']).toBe('Rare')
    expect(byName['Set code']).toBe('GRN')
    expect(byName['Mana value']).toBe(2) // a number, valid
    expect(byName['Color identity']).toBe('Black/Green')
    expect(props.every((p) => p.value !== '' && p.value != null)).toBe(true)
    // Absent facets are dropped, not emitted empty.
    expect('Power' in byName).toBe(false)
    expect('Language' in byName).toBe(false) // en is the default, so omitted
  })

  it('includes a non-default language and drops rarity when null', () => {
    const props = cardProductNode(makeCard({ lang: 'ja', rarity: null })).additionalProperty as {
      name: string
    }[]
    const names = props.map((p) => p.name)
    expect(names).toContain('Language')
    expect(names).not.toContain('Rarity')
  })

  it('joins both faces oracle text and strips mana braces in the description', () => {
    const node = cardProductNode(
      makeCard({
        oracle_text: null,
        faces: [
          {
            name: 'Front',
            mana_cost: '{G}',
            type_line: 'Creature',
            oracle_text: '{T}: Add {G}.',
            power: '1',
            toughness: '1',
            loyalty: null,
          },
          {
            name: 'Back',
            mana_cost: null,
            type_line: 'Land',
            oracle_text: 'Flying',
            power: null,
            toughness: null,
            loyalty: null,
          },
        ],
      }),
    )
    const description = node.description as string
    expect(description).toContain(' // ')
    expect(description).toContain('T: Add G.')
    expect(description).not.toContain('{')
  })
})

describe('sealedProductNode', () => {
  const linked = [
    makeComponent({ name: 'Collector Booster', product: makeProduct({ id: 'sub-1' }) }),
    makeComponent({
      kind: 'card',
      name: 'Foil Promo',
      quantity: 1,
      card: makeCard({ id: 'card-9' }),
    }),
    makeComponent({ kind: 'deck', name: 'Precon Deck', quantity: 1 }),
    makeComponent({ kind: 'other', name: 'Dice', quantity: 6 }),
  ]

  it('builds a valid Product with contents in the description and isRelatedTo', () => {
    const node = sealedProductNode(
      'mtg',
      makeProduct(),
      'Collector Booster Box',
      'Kaldheim',
      linked,
      'https://cdn/x.jpg',
    )
    expect(node['@type']).toBe('Product')
    expect(node.brand).toEqual({ '@type': 'Brand', name: 'Kaldheim' })
    expect(node.category).toBe('Collector Booster Box')
    expect(node.sku).toBe('prod-1')
    expect(node.description).toContain('12× Collector Booster')
    expect(node.description).toContain('1× Foil Promo')

    const related = node.isRelatedTo as { name: string; url: string }[]
    expect(related).toHaveLength(2) // only the product + card components; deck/other excluded
    expect(related.map((r) => r.url)).toEqual([
      `${ORIGIN}/sealed/mtg/sub-1`,
      `${ORIGIN}/cards/mtg/cards/card-9`,
    ])
  })

  it('omits the brand when no set name is known', () => {
    const node = sealedProductNode('mtg', makeProduct({ set_name: null }), 'Bundle', '', [])
    expect('brand' in node).toBe(false)
    expect('isRelatedTo' in node).toBe(false)
  })

  it('caps isRelatedTo at 20 entries', () => {
    const many = Array.from({ length: 25 }, (_, i) =>
      makeComponent({ name: `Pack ${i}`, product: makeProduct({ id: `p${i}` }) }),
    )
    const node = sealedProductNode('mtg', makeProduct(), 'Box', 'Set', many)
    expect((node.isRelatedTo as unknown[]).length).toBe(20)
  })
})

describe('no-storefront constraint guard', () => {
  // Neither node may claim purchasability — the deliberate price-tracker (not storefront)
  // stance. This fails the build if offers/availability/rating markup ever leaks back in.
  const banned = [
    'offers',
    'availability',
    'aggregaterating',
    '"review"',
    'pricecurrency',
    'haspart',
    'aggregateoffer',
  ]

  it('the card node emits no offer/availability/rating markup', () => {
    const s = JSON.stringify(cardProductNode(makeCard(), 'https://cdn/x.jpg')).toLowerCase()
    for (const b of banned) expect(s).not.toContain(b)
  })

  it('the sealed node emits no offer/availability/rating markup', () => {
    const node = sealedProductNode('mtg', makeProduct(), 'Box', 'Kaldheim', [
      makeComponent({ product: makeProduct({ id: 'x' }) }),
    ])
    const s = JSON.stringify(node).toLowerCase()
    for (const b of banned) expect(s).not.toContain(b)
  })
})

describe('breadcrumbs', () => {
  it('cardCrumbs is a 4-level trail through the set page, terminating on the card', () => {
    const crumbs = cardCrumbs('mtg', makeCard())
    expect(crumbs.map((c) => c.label)).toEqual([
      'Home',
      'Cards',
      'Guilds of Ravnica',
      "Assassin's Trophy",
    ])
    // Set page linked at index 2; the terminal card crumb has no link.
    expect(crumbs.map((c) => c.to)).toEqual([
      '/',
      '/cards/mtg/cards',
      '/cards/mtg/sets/grn',
      undefined,
    ])
  })

  it('sealedCrumbs is a 3-level trail (no set level)', () => {
    const crumbs = sealedCrumbs('mtg', makeProduct())
    expect(crumbs.map((c) => c.label)).toEqual(['Home', 'Sealed', 'Kaldheim Collector Booster Box'])
    expect(crumbs.map((c) => c.to)).toEqual(['/', '/sealed/mtg', undefined])
  })

  it('breadcrumbList emits contiguous ListItems with absolute item URLs, terminal item omitted', () => {
    const bl = breadcrumbList(cardCrumbs('mtg', makeCard()))
    expect(bl['@type']).toBe('BreadcrumbList')
    const items = bl.itemListElement as {
      '@type': string
      position: number
      name: string
      item?: string
    }[]
    expect(items.map((i) => i.position)).toEqual([1, 2, 3, 4])
    expect(items.every((i) => i['@type'] === 'ListItem')).toBe(true)
    expect(items.map((i) => i.item)).toEqual([
      `${ORIGIN}/`,
      `${ORIGIN}/cards/mtg/cards`,
      `${ORIGIN}/cards/mtg/sets/grn`,
      undefined,
    ])
    // The terminal crumb omits `item` entirely (valid per Google), not just sets it undefined.
    expect(items.filter((i) => 'item' in i)).toHaveLength(3)
  })
})

describe('graph', () => {
  it('wraps present nodes in one schema.org @graph in order', () => {
    const g = graph({ '@type': 'Product' }, { '@type': 'BreadcrumbList' })
    expect(g).toEqual({
      '@context': 'https://schema.org',
      '@graph': [{ '@type': 'Product' }, { '@type': 'BreadcrumbList' }],
    })
    // A single top-level @context (Google requirement) — the nodes carry none.
    expect(JSON.stringify(g).match(/@context/g)).toHaveLength(1)
  })

  it('drops nullish nodes and returns undefined when every node is absent', () => {
    expect(graph(null, { '@type': 'Product' })).toEqual({
      '@context': 'https://schema.org',
      '@graph': [{ '@type': 'Product' }],
    })
    expect(graph(null, undefined)).toBeUndefined()
  })
})
