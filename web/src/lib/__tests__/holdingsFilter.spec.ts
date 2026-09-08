import { describe, it, expect } from 'vitest'

import {
  COPIES_PRESETS,
  EMPTY_COPIES_FILTER,
  FINISH_OPTIONS,
  copiesFilterParams,
  copiesToken,
  describeCopiesFilter,
  isCopiesFilterActive,
  parseCopiesToken,
  parseFinish,
} from '../holdingsFilter'

describe('parseCopiesToken', () => {
  it('reads an exact count as a closed bound', () => {
    expect(parseCopiesToken('4')).toEqual({ min: 4, max: 4 })
    expect(parseCopiesToken('0')).toEqual({ min: 0, max: 0 })
  })

  it('reads a range, an at-least and an at-most token', () => {
    expect(parseCopiesToken('2-3')).toEqual({ min: 2, max: 3 })
    expect(parseCopiesToken('5-')).toEqual({ min: 5 })
    expect(parseCopiesToken('-3')).toEqual({ max: 3 })
  })

  it('trims surrounding whitespace', () => {
    expect(parseCopiesToken('  2-3  ')).toEqual({ min: 2, max: 3 })
  })

  it('treats a trailing + (or the space it decodes to) as "at least"', () => {
    // A hand-typed `?copies=5+` reaches us as `5 ` — a bare `+` in a query string is a space.
    expect(parseCopiesToken('5+')).toEqual({ min: 5 })
    expect(parseCopiesToken('5 ')).toEqual({ min: 5 })
    expect(parseCopiesToken('5-+')).toEqual({ min: 5 })
  })

  it('drops anything unparseable rather than filtering on junk', () => {
    expect(parseCopiesToken(undefined)).toEqual({})
    expect(parseCopiesToken('')).toEqual({})
    expect(parseCopiesToken('   ')).toEqual({})
    expect(parseCopiesToken('-')).toEqual({})
    expect(parseCopiesToken('four')).toEqual({})
    expect(parseCopiesToken('1-2-3')).toEqual({})
    expect(parseCopiesToken('2.5')).toEqual({})
  })

  it('rejects a negative bound and an inverted range', () => {
    expect(parseCopiesToken('-2-3')).toEqual({})
    expect(parseCopiesToken('5-2')).toEqual({})
  })
})

describe('copiesToken', () => {
  it('formats each bound shape canonically', () => {
    expect(copiesToken(4, 4)).toBe('4')
    expect(copiesToken(2, 3)).toBe('2-3')
    expect(copiesToken(5, undefined)).toBe('5-')
    expect(copiesToken(undefined, 3)).toBe('-3')
  })

  it('is undefined when unbounded or incoherent, so the URL key drops', () => {
    expect(copiesToken()).toBeUndefined()
    expect(copiesToken(undefined, undefined)).toBeUndefined()
    expect(copiesToken(5, 2)).toBeUndefined()
  })

  it('round-trips every canonical token', () => {
    for (const token of ['0', '1', '4', '2-3', '5-', '-3']) {
      const bounds = parseCopiesToken(token)
      expect(copiesToken(bounds.min, bounds.max)).toBe(token)
    }
  })

  it('round-trips the tolerated spellings onto their canonical form', () => {
    const bounds = parseCopiesToken('5+')
    expect(copiesToken(bounds.min, bounds.max)).toBe('5-')
  })

  it('round-trips every preset the chip offers', () => {
    for (const preset of COPIES_PRESETS) {
      const bounds = parseCopiesToken(preset.value)
      expect(copiesToken(bounds.min, bounds.max) ?? '').toBe(preset.value)
    }
  })
})

describe('parseFinish', () => {
  it('reads the two narrowing tokens and defaults everything else to any', () => {
    expect(parseFinish('regular')).toBe('regular')
    expect(parseFinish('foil')).toBe('foil')
    expect(parseFinish('any')).toBe('any')
    expect(parseFinish(undefined)).toBe('any')
    expect(parseFinish('etched')).toBe('any')
    expect(parseFinish(['foil'])).toBe('any')
  })
})

describe('isCopiesFilterActive', () => {
  it('is false only for the empty filter', () => {
    expect(isCopiesFilterActive(EMPTY_COPIES_FILTER)).toBe(false)
    expect(isCopiesFilterActive({ finish: 'foil' })).toBe(true)
    expect(isCopiesFilterActive({ min: 4, finish: 'any' })).toBe(true)
    expect(isCopiesFilterActive({ max: 3, finish: 'any' })).toBe(true)
    // A zero bound is a real filter ("cards I hold none of"), not an absent one.
    expect(isCopiesFilterActive({ min: 0, max: 0, finish: 'any' })).toBe(true)
  })
})

describe('copiesFilterParams', () => {
  it('omits everything for the empty filter', () => {
    expect(copiesFilterParams(EMPTY_COPIES_FILTER)).toEqual({})
  })

  it('sends both bounds and omits the default finish', () => {
    expect(copiesFilterParams({ min: 2, max: 3, finish: 'any' })).toEqual({
      minCopies: 2,
      maxCopies: 3,
    })
  })

  it('sends a narrowed finish, with or without bounds', () => {
    expect(copiesFilterParams({ finish: 'foil' })).toEqual({ finish: 'foil' })
    expect(copiesFilterParams({ min: 4, finish: 'foil' })).toEqual({
      minCopies: 4,
      finish: 'foil',
    })
  })

  it('keeps a zero bound (0 is meaningful, not absent)', () => {
    expect(copiesFilterParams({ max: 0, finish: 'any' })).toEqual({ maxCopies: 0 })
  })
})

describe('describeCopiesFilter', () => {
  it('is null while nothing is filtered', () => {
    expect(describeCopiesFilter(EMPTY_COPIES_FILTER)).toBeNull()
  })

  it('words each bound shape', () => {
    expect(describeCopiesFilter({ min: 1, max: 1, finish: 'any' })).toBe('1 copy')
    expect(describeCopiesFilter({ min: 2, max: 3, finish: 'any' })).toBe('2–3 copies')
    expect(describeCopiesFilter({ min: 4, max: 4, finish: 'any' })).toBe('4 copies')
    expect(describeCopiesFilter({ min: 5, finish: 'any' })).toBe('5 or more copies')
    expect(describeCopiesFilter({ max: 3, finish: 'any' })).toBe('up to 3 copies')
  })

  it('words a bare finish filter as "any of this finish"', () => {
    expect(describeCopiesFilter({ finish: 'foil' })).toBe('foil copies')
    expect(describeCopiesFilter({ finish: 'regular' })).toBe('regular copies')
  })

  it('folds the finish into a bounded phrase', () => {
    expect(describeCopiesFilter({ min: 4, max: 4, finish: 'foil' })).toBe('4 foil copies')
    expect(describeCopiesFilter({ min: 5, finish: 'foil' })).toBe('5 or more foil copies')
    expect(describeCopiesFilter({ min: 2, max: 3, finish: 'regular' })).toBe('2–3 regular copies')
    expect(describeCopiesFilter({ min: 1, max: 1, finish: 'foil' })).toBe('1 foil copy')
  })

  it('describes every preset the chip offers', () => {
    const described = COPIES_PRESETS.map((preset) =>
      describeCopiesFilter({ ...parseCopiesToken(preset.value), finish: 'any' }),
    )
    expect(described).toEqual([null, '1 copy', '2–3 copies', '4 copies', '5 or more copies'])
  })
})

describe('the chip option lists', () => {
  it('offers the copy presets in ladder order, led by the no-filter option', () => {
    expect(COPIES_PRESETS.map((option) => option.value)).toEqual(['', '1', '2-3', '4', '5-'])
  })

  it('offers every finish token the API accepts, led by the default', () => {
    expect(FINISH_OPTIONS.map((option) => option.value)).toEqual(['any', 'regular', 'foil'])
  })
})
