import { describe, it, expect } from 'vitest'

import { boosterFamilyLabel, productTypeLabel } from '../productType'

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

describe('boosterFamilyLabel', () => {
  it('labels booster slugs by family (pack + box share one label)', () => {
    expect(boosterFamilyLabel('collector_pack')).toBe('Collector Booster')
    expect(boosterFamilyLabel('collector_display')).toBe('Collector Booster')
    expect(boosterFamilyLabel('play_pack')).toBe('Play Booster')
    expect(boosterFamilyLabel('set_display')).toBe('Set Booster')
    expect(boosterFamilyLabel('draft_pack')).toBe('Draft Booster')
    // A generic booster line (Jumpstart, Mystery Booster) has no family keyword.
    expect(boosterFamilyLabel('pack')).toBe('Booster')
    expect(boosterFamilyLabel('display')).toBe('Booster')
  })

  it('returns null for a non-booster product (no exclusives section)', () => {
    expect(boosterFamilyLabel('bundle')).toBeNull()
    expect(boosterFamilyLabel('commander_deck')).toBeNull()
    expect(boosterFamilyLabel('case')).toBeNull()
    expect(boosterFamilyLabel('secret_lair')).toBeNull()
    expect(boosterFamilyLabel('')).toBeNull()
  })
})
