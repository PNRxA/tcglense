import { describe, it, expect } from 'vitest'

import { finishLabel, frameEffectLabel, promoTypeLabel, sentenceCase } from '@/lib/printDetails'

// The card page's print details arrive as bare Scryfall slugs; these are the labels that
// make them readable, plus the fallback that keeps an unmapped slug renderable (issue #673).
describe('print-detail labels', () => {
  it('spells the known frame effects', () => {
    expect(frameEffectLabel('extendedart')).toBe('Extended art')
    expect(frameEffectLabel('showcase')).toBe('Showcase')
    expect(frameEffectLabel('legendary')).toBe('Legendary crown')
  })

  it('falls back to sentence case for an unknown frame effect', () => {
    // Upstream adds effects with every set — an unmapped one must still render, and read
    // as a name rather than as a raw slug.
    expect(frameEffectLabel('sparkleframe')).toBe('Sparkleframe')
    expect(frameEffectLabel('some-new-effect')).toBe('Some new effect')
  })

  it('spells the known promo types, foil treatments included', () => {
    expect(promoTypeLabel('buyabox')).toBe('Buy-a-Box')
    expect(promoTypeLabel('sldbonus')).toBe('Secret Lair bonus')
    expect(promoTypeLabel('stepandcompleat')).toBe('Step-and-Compleat foil')
    expect(promoTypeLabel('fnm')).toBe('Friday Night Magic')
  })

  it('falls back to sentence case for an unknown promo type', () => {
    expect(promoTypeLabel('mysteryfoil')).toBe('Mysteryfoil')
  })

  it('names the three finishes as a player would', () => {
    expect(finishLabel('nonfoil')).toBe('Regular')
    expect(finishLabel('foil')).toBe('Foil')
    expect(finishLabel('etched')).toBe('Etched foil')
    expect(finishLabel('glossy')).toBe('Glossy')
  })

  it('sentence-cases a slug without inventing words', () => {
    expect(sentenceCase('oil_slick')).toBe('Oil slick')
    expect(sentenceCase('')).toBe('')
  })
})
