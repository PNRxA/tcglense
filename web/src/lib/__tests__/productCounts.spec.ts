import { describe, it, expect } from 'vitest'
import type { ProductCardSection, ProductComponent } from '@/lib/api'
import {
  boxItemCount,
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
