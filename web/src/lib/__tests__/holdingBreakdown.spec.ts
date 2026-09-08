import { describe, expect, it } from 'vitest'
import type { BreakdownBucket, HoldingBreakdown } from '@/lib/api'
import {
  BREAKDOWN_FACETS,
  breakdownBars,
  bucketColor,
  bucketLabel,
  hasBreakdown,
} from '../holdingBreakdown'

function bucket(key: string, value_usd: string | null, copies = 1): BreakdownBucket {
  return { key, cards: 1, copies, value_usd }
}

describe('bucketLabel', () => {
  it('words the server keys per facet', () => {
    expect(bucketLabel('rarity', 'mythic')).toBe('Mythic')
    expect(bucketLabel('rarity', 'unknown')).toBe('Unknown rarity')
    expect(bucketLabel('color', 'multicolor')).toBe('Multicolour')
    expect(bucketLabel('color', 'colorless')).toBe('Colourless')
    expect(bucketLabel('card_type', 'creature')).toBe('Creature')
    expect(bucketLabel('card_type', 'other')).toBe('Other')
    expect(bucketLabel('finish', 'foil')).toBe('Foil')
    expect(bucketLabel('finish', 'regular')).toBe('Regular')
  })

  it('renders a key it does not know as itself, capitalised', () => {
    expect(bucketLabel('rarity', 'bonus')).toBe('Bonus')
    expect(bucketLabel('color', 'ultraviolet')).toBe('Ultraviolet')
    expect(bucketLabel('card_type', 'battle')).toBe('Battle')
    // A finish the server might add later (etched) must not read as Regular.
    expect(bucketLabel('finish', 'etched')).toBe('Etched')
  })
})

describe('bucketColor', () => {
  it('draws rarity and finish through design-system tokens, never a palette literal', () => {
    expect(bucketColor('rarity', 'rare')).toBe('var(--rarity-rare)')
    expect(bucketColor('rarity', 'common')).toBe('var(--muted-foreground)')
    expect(bucketColor('rarity', 'special')).toBe('var(--primary)')
    expect(bucketColor('finish', 'foil')).toBe('var(--foil)')
    expect(bucketColor('finish', 'regular')).toBe('var(--primary)')
    expect(bucketColor('card_type', 'creature')).toBe('var(--primary)')
  })

  it('draws colour identity in the mana swatches', () => {
    expect(bucketColor('color', 'blue')).toBe('#3b82f6')
    expect(bucketColor('color', 'nope')).toBe('var(--primary)')
  })
})

describe('breakdownBars', () => {
  it('keeps the server order and shares each bar against the facet total', () => {
    const bars = breakdownBars('rarity', [
      bucket('common', '10.00', 40),
      bucket('rare', '30.00', 2),
      bucket('unknown', null, 3),
    ])
    expect(bars.map((b) => b.key)).toEqual(['common', 'rare', 'unknown'])
    expect(bars[0]!.share).toBeCloseTo(0.25)
    expect(bars[1]!.share).toBeCloseTo(0.75)
    // An unpriced bucket is still a bar (its copies are real) at zero share.
    expect(bars[2]!.share).toBe(0)
    expect(bars[2]!.valueUsd).toBeNull()
    expect(bars[2]!.copies).toBe(3)
    expect(bars[1]!.label).toBe('Rare')
    expect(bars[1]!.color).toBe('var(--rarity-rare)')
  })

  it('draws every bar at zero when nothing in the facet is priced', () => {
    const bars = breakdownBars('finish', [bucket('regular', null), bucket('foil', null)])
    expect(bars.map((b) => b.share)).toEqual([0, 0])
  })
})

describe('hasBreakdown', () => {
  const empty: HoldingBreakdown = {
    summary: { unique_cards: 0, total_cards: 0, total_value_usd: null, bulk_value_usd: null },
    rarity: [],
    color: [],
    card_type: [],
    finish: [],
    top: [],
    unpriced_cards: 0,
  }

  it('is false for nothing and for an empty breakdown', () => {
    expect(hasBreakdown(undefined)).toBe(false)
    expect(hasBreakdown(empty)).toBe(false)
  })

  it('is true once any facet has a bucket', () => {
    expect(hasBreakdown({ ...empty, finish: [bucket('regular', null)] })).toBe(true)
  })

  it('offers the four facets in a fixed order', () => {
    expect(BREAKDOWN_FACETS.map((f) => f.key)).toEqual(['rarity', 'color', 'card_type', 'finish'])
  })
})
