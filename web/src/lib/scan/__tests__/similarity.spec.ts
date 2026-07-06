import { describe, it, expect } from 'vitest'
import {
  canonical,
  deconfuseDigits,
  levenshtein,
  namePoolPrefix,
  ocrSimilarity,
  rankNames,
} from '../similarity'

describe('canonical', () => {
  it('strips diacritics, case, spacing and punctuation', () => {
    expect(canonical('Lim-Dûl the Necromancer')).toBe('limdulthenecromancer')
    expect(canonical("Gaea's  Cradle")).toBe('gaeascradle')
  })

  it('keeps digits (they carry signal until scored)', () => {
    expect(canonical('Borrowing 100,000 Arrows')).toBe('borrowing100000arrows')
  })
})

describe('deconfuseDigits', () => {
  it('swaps digits an OCR likely misread for their letter twins', () => {
    expect(deconfuseDigits('Lightn1ng B0lt')).toBe('Lightning Bolt')
    expect(deconfuseDigits('5erra Angel')).toBe('serra Angel')
  })

  it('leaves letters untouched', () => {
    expect(deconfuseDigits('Serra Angel')).toBe('Serra Angel')
  })
})

describe('levenshtein', () => {
  it('counts single-glyph edits', () => {
    expect(levenshtein('neo', 'neo')).toBe(0)
    expect(levenshtein('ne0', 'neo')).toBe(1)
    expect(levenshtein('nxx', 'neo')).toBe(2)
    expect(levenshtein('', 'neo')).toBe(3)
  })
})

describe('ocrSimilarity', () => {
  it('is 1 for an identical read (after folding)', () => {
    expect(ocrSimilarity('Serra Angel', 'Serra Angel')).toBe(1)
    expect(ocrSimilarity('serra  angel', 'Serra Angel')).toBe(1)
  })

  it('barely dips for a confusable-glyph swap', () => {
    // '1' for 'i' and '0' for 'o' are cheap edits, so the score stays high.
    expect(ocrSimilarity('Lightn1ng Bolt', 'Lightning Bolt')).toBeGreaterThan(0.9)
    expect(ocrSimilarity('B0lt', 'Bolt')).toBeGreaterThan(0.9)
  })

  it('scores an unrelated name low', () => {
    expect(ocrSimilarity('Lightning Bolt', 'Serra Angel')).toBeLessThan(0.4)
  })

  it('is 0 when either side has no comparable characters', () => {
    expect(ocrSimilarity('', 'Bolt')).toBe(0)
    expect(ocrSimilarity('***', 'Bolt')).toBe(0)
  })
})

describe('rankNames', () => {
  it('orders candidates by closeness to the OCR read, best first', () => {
    const pool = ['Serra Avatar', 'Serra Angel', 'Serra Angel Avacyn']
    expect(rankNames('Serra Angel', pool)[0]).toBe('Serra Angel')
  })

  it('recovers the target despite a confusable misread', () => {
    const pool = ['Lightning Helix', 'Lightning Bolt', 'Lightning Axe']
    expect(rankNames('Lightn1ng Bolt', pool)[0]).toBe('Lightning Bolt')
  })

  it('keeps server order for ties (stable)', () => {
    const pool = ['Forest', 'Forest']
    expect(rankNames('zzzz', pool)).toEqual(['Forest', 'Forest'])
  })
})

describe('namePoolPrefix', () => {
  it('returns a short, digit-corrected leading slice', () => {
    expect(namePoolPrefix('Lightning Bolt')).toBe('Light')
    expect(namePoolPrefix('L1ghtning Bolt')).toBe('Light')
  })

  it('returns null when there is too little clean text to query', () => {
    expect(namePoolPrefix('ab')).toBeNull()
  })
})
