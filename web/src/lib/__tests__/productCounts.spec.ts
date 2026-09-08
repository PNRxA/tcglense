import { describe, it, expect } from 'vitest'
import type {
  PackEv,
  PackOpening,
  ProductCardSection,
  ProductComponent,
  ProductEv,
} from '@/lib/api'
import {
  boosterLabel,
  boxItemCount,
  cardsPerPackLabel,
  evVersusPrice,
  expectedValueHeading,
  oddsLabel,
  openingSummary,
  openingVersusPrice,
  productCardChips,
  productCardCounts,
  productCardsHeading,
  visibleProductSections,
} from '@/lib/productCounts'

const section = (key: string, total: number, boosterFamily: string | null = null) =>
  ({
    key,
    total,
    booster_family: boosterFamily,
    component: null,
    inherited: false,
  }) as ProductCardSection

const inheritedSection = (key: string, total: number) => ({
  ...section(key, total),
  inherited: true,
})

const componentSection = (key: string, name: string, total: number) => ({
  ...section(key, total),
  component: name,
})

const component = (name: string, quantity: number) =>
  ({ kind: 'sealed', name, quantity, product: null, card: null }) as ProductComponent

const counts = (manifest: ProductCardSection[]) => productCardCounts(manifest)

describe('productCardCounts', () => {
  it('splits the manifest by certainty', () => {
    expect(
      counts([
        section('contains', 3),
        section('exclusive', 8),
        section('booster', 52),
        section('variable', 2),
      ]),
    ).toEqual({
      guaranteed: 3,
      pool: 60,
      exclusive: 8,
      variable: 2,
      possible: 62,
      total: 65,
    })
  })

  it('treats exclusives as a subset of the pool, never as extra cards', () => {
    // 8 exclusive + 52 shared is a 60-card pool, not 68 — the exclusives chip and the pool
    // chip must not be addable.
    const c = counts([section('exclusive', 8), section('booster', 52)])
    expect(c.pool).toBe(60)
    expect(c.total).toBe(60)
    expect(c.exclusive).toBeLessThan(c.pool)
  })

  it('files an unrecognised key into the weakest bucket, mirroring the server', () => {
    const c = counts([section('someday', 4)])
    expect(c.variable).toBe(4)
    expect(c.guaranteed).toBe(0)
    expect(c.possible).toBe(4)
  })

  it('is all zeroes for an empty manifest (a search matching nothing)', () => {
    expect(counts([])).toMatchObject({ guaranteed: 0, pool: 0, variable: 0, total: 0 })
  })

  it('folds a component section into the certainty bucket its key names', () => {
    // What the land pack guarantees, the box guarantees; what a pack pool offers stays a pool.
    const c = counts([
      componentSection('contains', 'Land Pack', 5),
      componentSection('variable', 'Land Pack', 1),
      section('contains', 2),
    ])
    expect(c.guaranteed).toBe(7)
    expect(c.variable).toBe(1)
    expect(c.total).toBe(8)
  })
})

describe('visibleProductSections', () => {
  it('drops an inherited booster/exclusive section — the pool lives on the linked child’s page', () => {
    const visible = visibleProductSections([
      section('contains', 2),
      inheritedSection('exclusive', 80),
      inheritedSection('booster', 520),
      section('variable', 1),
    ])
    expect(visible.map((s) => s.key)).toEqual(['contains', 'variable'])
  })

  it('keeps a non-inherited pool, an inherited guarantee, and every component section', () => {
    const visible = visibleProductSections([
      inheritedSection('contains', 3),
      componentSection('contains', 'Land Pack', 5),
      section('booster', 600),
    ])
    expect(visible).toHaveLength(3)
  })

  it('keeps everything when nothing is flagged (a pack page, a precon)', () => {
    const manifest = [section('exclusive', 8), section('booster', 52)]
    expect(visibleProductSections(manifest)).toEqual(manifest)
  })
})

describe('productCardsHeading', () => {
  const heading = (manifest: ProductCardSection[], filtered = false) =>
    productCardsHeading(counts(manifest), filtered)

  it('keeps the containment wording only when nothing is random', () => {
    expect(heading([section('contains', 99)])).toEqual({
      title: 'Cards in this product',
      count: '(99)',
      blurb: '',
    })
  })

  it('never claims containment over a pure pull pool', () => {
    // The reported bug, at its real scale: a booster's ~600-card pool is not its contents.
    const h = heading([section('booster', 600)])
    expect(h.title).toBe('What you can pull')
    expect(h.count).toBe('(600-card pool)')
    expect(h.blurb).toBe('')
  })

  it('hedges anything randomized with one voice', () => {
    expect(heading([section('variable', 12)])).toEqual({
      title: 'What you might get',
      count: '(12)',
      blurb: '',
    })
  })

  it('keeps the pool legible when a randomized insert joins it', () => {
    // A collector box is routinely a big pull pool plus a randomized foil insert and nothing
    // guaranteed. Without the split line this collapses to "(602)" — the original bug, back.
    expect(heading([section('booster', 600), section('variable', 2)])).toEqual({
      title: 'What you might get',
      count: '(602)',
      blurb:
        '600 in the pull pool · 2 sometimes included — a copy opens some of the pool, not all of it.',
    })
  })

  it('drops the pool-size claim while a search narrows the manifest', () => {
    // Unfiltered the number IS the pool; filtered it is only what matched, so asserting
    // "(12-card pool)" over a 600-card pool would be a brand-new lie.
    expect(heading([section('booster', 600)], false).count).toBe('(600-card pool)')
    expect(heading([section('booster', 12)], true).count).toBe('(12)')
    expect(heading([section('booster', 12)], true).title).toBe('What you can pull')
  })

  it('claims no certainty at all when a search matched nothing', () => {
    expect(heading([], true)).toEqual({ title: 'Cards', count: '(0)', blurb: '' })
  })

  it('names both certainties on a mixed product and spells the split out', () => {
    // A real bundle is 1 promo + a ~600-card pool; "(601)" alone would hide the pool inside a
    // number that looks like contents.
    expect(heading([section('contains', 1), section('booster', 600)])).toEqual({
      title: "What's guaranteed, what's random",
      count: '(601)',
      blurb: '1 guaranteed · 600 in the pull pool — a copy opens some of the pool, not all of it.',
    })
  })

  it('omits the pool clause from the mixed line when there is no pool', () => {
    expect(heading([section('contains', 2), section('variable', 1)]).blurb).toBe(
      '2 guaranteed · 1 sometimes included.',
    )
  })

  it('reads correctly at one, with no singular/plural inflection needed', () => {
    expect(heading([section('booster', 1)]).count).toBe('(1-card pool)')
    expect(heading([section('contains', 1)]).count).toBe('(1)')
    expect(heading([section('variable', 1)]).count).toBe('(1)')
  })

  it('groups thousands so a real pool stays readable', () => {
    expect(heading([section('booster', 1234)]).count).toBe('(1,234-card pool)')
  })
})

describe('productCardChips', () => {
  it('splits a bundle into one chip per certainty, over disjoint counts', () => {
    const chips = productCardChips(
      counts([section('contains', 3), section('booster', 52), section('variable', 2)]),
      null,
    )
    expect(chips.map((c) => [c.id, c.count, c.label])).toEqual([
      ['guaranteed', 3, 'guaranteed cards'],
      ['pull', 52, 'cards in the pull pool'],
      ['variable', 2, 'cards it might include'],
    ])
  })

  it('never labels a pull pool as cards inside', () => {
    const chips = productCardChips(counts([section('booster', 600)]), null)
    expect(chips).toHaveLength(1)
    expect(chips[0]?.label).toBe('cards in the pull pool')
    expect(chips.some((c) => c.label.includes('inside'))).toBe(false)
    // "cards you can pull" would parse as "you can pull 600 cards" — the original lie.
    expect(chips.some((c) => c.label.includes('you can pull'))).toBe(false)
  })

  it('names the exclusive slice as part of the pool the chip before it counts', () => {
    const chips = productCardChips(
      counts([section('exclusive', 8, 'collector_pack'), section('booster', 52)]),
      'Collector Booster',
    )
    expect(chips.map((c) => c.id)).toEqual(['pull', 'exclusive'])
    expect(chips[0]?.count).toBe(60)
    // Self-contained: "of them" would lose its antecedent the moment the flex row wrapped.
    expect(chips[1]?.label).toBe('of the pool, exclusive to Collector Booster')
    expect(chips.every((c) => !c.label.includes('of them'))).toBe(true)
  })

  it('goes generic when the backend names no booster family', () => {
    const chips = productCardChips(counts([section('exclusive', 8), section('booster', 52)]), null)
    expect(chips[1]?.label).toBe("of the pool, exclusive to this product's boosters")
  })

  it('drops the exclusives chip when the whole pool is exclusive (it would restate it)', () => {
    const chips = productCardChips(counts([section('exclusive', 8)]), 'Collector Booster')
    expect(chips.map((c) => c.id)).toEqual(['pull'])
  })

  it('emits no chip for an empty certainty', () => {
    expect(productCardChips(counts([]), null)).toEqual([])
  })

  it('inflects each label at one', () => {
    const chips = productCardChips(
      counts([section('contains', 1), section('booster', 1), section('variable', 1)]),
      null,
    )
    expect(chips.map((c) => c.label)).toEqual([
      'guaranteed card',
      'card in the pull pool',
      'card it might include',
    ])
  })
})

describe('boxItemCount', () => {
  it('sums the quantities rather than counting line items', () => {
    // A booster box is 30 packs plus a topper — 31 pieces, not 2 rows.
    expect(boxItemCount([component('Play Booster', 30), component('Box Topper', 1)])).toBe(31)
  })

  it('is zero for an unknown composition', () => {
    expect(boxItemCount([])).toBe(0)
  })

  it('lets a malformed row contribute nothing, so the count never exceeds the rows shown', () => {
    // The count exists to agree with the `N×` the rows render; counting a `0×` row as an item
    // would break it in exactly the case a clamp was meant to cover.
    expect(boxItemCount([component('Odd row', 0)])).toBe(0)
    expect(boxItemCount([component('Pack', 3), component('Odd row', 0)])).toBe(3)
  })
})

// ---------- Booster expected value + the seeded opener (issue #682) ----------

const packEv = (overrides: Partial<PackEv> = {}): PackEv => ({
  set_code: 'blb',
  booster_code: 'play',
  name: 'Play Booster',
  quantity: 1,
  cards_per_pack: 14,
  ev_usd: '5.00',
  priced_share: 1,
  slots: [],
  top: [],
  ...overrides,
})

const productEv = (packs: PackEv[], overrides: Partial<ProductEv> = {}): ProductEv => ({
  ev_usd: '5.00',
  packs,
  top: [],
  caveats: [],
  ...overrides,
})

const opening = (overrides: Partial<PackOpening> = {}): PackOpening => ({
  seed: 7,
  copies: 1,
  packs: [],
  value_usd: '12.34',
  priced_count: 14,
  unpriced_count: 0,
  caveats: [],
  ...overrides,
})

describe('expectedValueHeading', () => {
  it('quotes a single booster per pack', () => {
    const heading = expectedValueHeading(productEv([packEv({ quantity: 1 })]))
    expect(heading.title).toBe('Expected value')
    expect(heading.unit).toBe('per pack, on average')
    // Nothing to reconcile: one copy IS one pack, so the blurb is only the qualification.
    expect(heading.blurb).toBe(
      "An average over many openings at today's prices — not what any one pack holds.",
    )
  })

  it('quotes a box per copy and spells out how many packs a copy opens', () => {
    const heading = expectedValueHeading(productEv([packEv({ quantity: 36 })]))
    expect(heading.unit).toBe('per copy, on average')
    expect(heading.blurb).toContain('One copy opens 36 packs.')
  })

  it('sums the packs a copy opens across booster configurations', () => {
    // A bundle: nine play boosters plus a collector booster is 10 packs, not 2 lines.
    const heading = expectedValueHeading(
      productEv([packEv({ quantity: 9 }), packEv({ booster_code: 'collector', quantity: 1 })]),
    )
    expect(heading.blurb).toContain('One copy opens 10 packs.')
  })

  it('never claims a copy opens zero packs', () => {
    const heading = expectedValueHeading(productEv([]))
    expect(heading.blurb).not.toContain('0 packs')
    expect(heading.unit).toBe('per copy, on average')
  })

  it('always says the figure is an average at today’s prices', () => {
    for (const quantity of [1, 6, 36]) {
      const heading = expectedValueHeading(productEv([packEv({ quantity })]))
      expect(heading.blurb).toContain("An average over many openings at today's prices")
      expect(heading.blurb).toContain('not what any one pack holds')
    }
  })
})

describe('evVersusPrice', () => {
  it('reads the expectation as a share of the current price', () => {
    const vs = evVersusPrice('6.20', '10.00')
    expect(vs?.ratio).toBeCloseTo(0.62)
    expect(vs?.label).toBe('Expected value is 62% of the current price')
  })

  it('keeps the comparison free of advice', () => {
    const label = evVersusPrice('120.00', '100.00')!.label
    // An expectation over many openings is not a promise about this copy: nothing here may
    // read as money back, a profit, or a recommendation.
    expect(label).not.toMatch(/get back|profit|worth buying|value for money/i)
    expect(label).toBe('Expected value is 120% of the current price')
  })

  it('is null without a usable price to compare against', () => {
    expect(evVersusPrice('6.20', null)).toBeNull()
    expect(evVersusPrice('6.20', undefined)).toBeNull()
    expect(evVersusPrice('6.20', '')).toBeNull()
    expect(evVersusPrice('6.20', '0')).toBeNull()
    expect(evVersusPrice('6.20', 'not a price')).toBeNull()
  })
})

describe('oddsLabel', () => {
  it('quotes whole packs above ten', () => {
    expect(oddsLabel(24)).toBe('1 in 24 packs')
    // Rounded, not truncated — and never to a decimal at this end, where the
    // with-replacement approximation can't support one.
    expect(oddsLabel(23.7)).toBe('1 in 24 packs')
    expect(oddsLabel(900)).toBe('1 in 900 packs')
  })

  it('keeps one decimal below ten, where the fraction is the answer', () => {
    expect(oddsLabel(2.5)).toBe('1 in 2.5 packs')
    expect(oddsLabel(2)).toBe('1 in 2 packs')
    expect(oddsLabel(9.44)).toBe('1 in 9.4 packs')
  })

  it('stops quoting a ratio once it would read as a guarantee', () => {
    expect(oddsLabel(1.5)).toBe('most packs')
    expect(oddsLabel(1.2)).toBe('most packs')
    expect(oddsLabel(1)).toBe('most packs')
  })

  it('says nothing precise about an unusable figure', () => {
    expect(oddsLabel(Number.POSITIVE_INFINITY)).toBe('rarely')
    expect(oddsLabel(Number.NaN)).toBe('rarely')
    expect(oddsLabel(0)).toBe('rarely')
  })
})

describe('cardsPerPackLabel', () => {
  it('words the sheet count per pack, never as the product’s contents', () => {
    expect(cardsPerPackLabel(14)).toBe('14 cards per pack')
    expect(cardsPerPackLabel(1)).toBe('1 card per pack')
  })

  it('renders a fractional expectation as the range it really is', () => {
    // A configuration that sometimes deals a fifteenth card averages 14.25 — a pack is
    // never "14.25 cards", so say what it can be.
    expect(cardsPerPackLabel(14.25)).toBe('14–15 cards per pack')
    expect(cardsPerPackLabel(15.999)).toBe('16 cards per pack')
  })

  it('says nothing when there is no count to state', () => {
    expect(cardsPerPackLabel(0)).toBe('')
    expect(cardsPerPackLabel(Number.NaN)).toBe('')
  })
})

describe('openingSummary', () => {
  it('words the total as one run of the dice', () => {
    const summary = openingSummary(opening({ packs: [{}] as PackOpening['packs'] }))
    expect(summary.title).toBe('What this run dealt')
    expect(summary.value).toBe('pulled in this run')
    expect(summary.blurb).toBe('This run is one roll of the dice, not what a pack is worth.')
  })

  it('names the run’s size once it spans more than one pack', () => {
    const summary = openingSummary(opening({ packs: Array(6).fill({}) as PackOpening['packs'] }))
    expect(summary.title).toBe('What this run dealt across 6 packs')
  })

  it('says how much of the run had no price, so the total is never read as complete', () => {
    const summary = openingSummary(opening({ priced_count: 12, unpriced_count: 3 }))
    expect(summary.blurb).toContain('3 of the 15 cards dealt had no market price')
    expect(summary.blurb).toContain('count as $0')
  })
})

describe('the EV + opener vocabulary as a whole', () => {
  // Every string the two panels can print, over a spread of inputs.
  const strings = (): string[] => {
    const out: string[] = []
    for (const quantity of [1, 6, 36]) {
      const heading = expectedValueHeading(productEv([packEv({ quantity })]))
      out.push(heading.title, heading.unit, heading.blurb)
    }
    out.push(evVersusPrice('6.20', '10.00')!.label)
    out.push(openingVersusPrice(opening(), '10.00')!)
    out.push(openingVersusPrice(opening({ copies: 6 }), '10.00')!)
    for (const oneIn of [1, 2.5, 24, 900, Number.POSITIVE_INFINITY]) out.push(oddsLabel(oneIn))
    for (const per of [1, 14, 14.25]) out.push(cardsPerPackLabel(per))
    for (const unpriced of [0, 3]) {
      const summary = openingSummary(
        opening({ unpriced_count: unpriced, packs: Array(6).fill({}) as PackOpening['packs'] }),
      )
      out.push(summary.title, summary.value, summary.blurb)
    }
    return out
  }

  it('never words a number as the product’s contents or a guarantee', () => {
    // The rule this whole module exists for: an expectation and a simulation are not
    // containment. `contains` catches the manifest's own vocabulary leaking in here.
    for (const text of strings()) {
      expect(text.toLowerCase()).not.toContain('cards in this product')
      expect(text.toLowerCase()).not.toContain('guaranteed')
      expect(text.toLowerCase()).not.toContain('contains')
    }
  })

  it('qualifies every money-bearing string as an average, an expectation, or this run', () => {
    // The strings that sit next to a dollar figure are the ones that can be read as a
    // promise, so each has to carry its own qualification — a caption read on its own
    // (a screen reader, a wrapped row) must still say what kind of number it labels.
    const moneyStrings = [
      ...[1, 36].flatMap((quantity) => {
        const heading = expectedValueHeading(productEv([packEv({ quantity })]))
        return [heading.title, heading.unit, heading.blurb]
      }),
      evVersusPrice('6.20', '10.00')!.label,
      openingVersusPrice(opening({ copies: 6 }), '10.00')!,
      ...(() => {
        const summary = openingSummary(opening({ unpriced_count: 3 }))
        return [summary.title, summary.value, summary.blurb]
      })(),
    ]
    for (const text of moneyStrings) {
      expect(text).toMatch(/average|expected|this run/i)
    }
  })
})

describe('boosterLabel', () => {
  it('prefers the booster’s stated name', () => {
    expect(boosterLabel({ name: 'Play Booster', booster_code: 'play' })).toBe('Play Booster')
  })

  it('falls back to upstream’s own key when it states none', () => {
    expect(boosterLabel({ name: null, booster_code: 'collector' })).toBe('collector booster')
  })
})

describe('openingVersusPrice', () => {
  it('reads the run’s total as a share of what one copy costs', () => {
    expect(openingVersusPrice(opening({ value_usd: '5.00' }), '10.00')).toBe(
      "This run dealt 50% of what one copy costs at today's price",
    )
  })

  it('scales the comparison by the copies opened', () => {
    // Six copies at $10 is a $60 run, not a $10 one.
    expect(openingVersusPrice(opening({ value_usd: '30.00', copies: 6 }), '10.00')).toBe(
      "This run dealt 50% of what 6 copies cost at today's price",
    )
  })

  it('keeps the subject the run, never the product’s worth', () => {
    const label = openingVersusPrice(opening({ value_usd: '400.00' }), '10.00')!
    expect(label).toContain('This run dealt')
    expect(label).not.toMatch(/get back|profit|worth/i)
  })

  it('is null without a usable price', () => {
    expect(openingVersusPrice(opening(), null)).toBeNull()
    expect(openingVersusPrice(opening(), '0')).toBeNull()
  })
})
