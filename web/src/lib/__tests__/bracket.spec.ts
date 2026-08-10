import { describe, expect, it } from 'vitest'
import {
  BRACKET_BAR,
  BRACKET_TONE,
  bracketBar,
  bracketTone,
  ESTIMATABLE_BRACKETS,
} from '../bracket'

// The ladder itself ships with every estimate (see `analysis::bracket::LADDER`), so there is
// nothing to mirror here — but the panel draws a rung per bracket the *server* sends, so a
// missing tone would render an unstyled segment rather than fail a build. These pin the one
// thing that isn't type-checked: full coverage of brackets 1–5.

describe('bracket presentation', () => {
  it('has a tone and a bar for every rung of the ladder', () => {
    for (const bracket of [1, 2, 3, 4, 5]) {
      expect(BRACKET_TONE[bracket], `tone for bracket ${bracket}`).toBeTruthy()
      expect(BRACKET_BAR[bracket], `bar for bracket ${bracket}`).toBeTruthy()
    }
    expect(Object.keys(BRACKET_TONE)).toHaveLength(5)
    expect(Object.keys(BRACKET_BAR)).toHaveLength(5)
  })

  it('falls back to a neutral chip rather than rendering unstyled', () => {
    expect(bracketTone(0)).toContain('bg-muted')
    expect(bracketBar(9)).toContain('bg-muted-foreground')
    expect(bracketTone(3)).toBe(BRACKET_TONE[3])
    expect(bracketBar(3)).toBe(BRACKET_BAR[3])
  })

  it('marks only the brackets a decklist can establish', () => {
    // 1 (Exhibition) and 5 (cEDH) are claims about intent — `analyse_bracket` never returns
    // either, so the ladder must not present them as ruled out.
    expect([...ESTIMATABLE_BRACKETS]).toEqual([2, 3, 4])
  })
})
