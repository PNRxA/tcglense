import { describe, it, expect } from 'vitest'

import { productTypeLabel } from '../productType'

describe('productTypeLabel', () => {
  it('maps known slugs to readable labels', () => {
    expect(productTypeLabel('collector_display')).toBe('Collector Booster Box')
    expect(productTypeLabel('play_pack')).toBe('Play Booster Pack')
    expect(productTypeLabel('commander_deck')).toBe('Commander Deck')
    expect(productTypeLabel('secret_lair')).toBe('Secret Lair')
    expect(productTypeLabel('bundle')).toBe('Bundle')
    expect(productTypeLabel('other')).toBe('Other')
  })

  it('humanises an unknown slug rather than showing it raw', () => {
    expect(productTypeLabel('mystery_box')).toBe('Mystery Box')
    expect(productTypeLabel('foo')).toBe('Foo')
  })

  it('is stable on empty input', () => {
    expect(productTypeLabel('')).toBe('')
  })
})
