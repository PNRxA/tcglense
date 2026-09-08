import { describe, expect, it } from 'vitest'
import type { DeckDiffEntry } from '@/lib/api'
import {
  DECK_DIFF_CHANGES,
  countsLabel,
  deltaLabel,
  foilLabel,
  groupByChange,
  summaryLabel,
} from '@/lib/deckDiff'

// The fold is the server's; these pin the strings the compare panel prints from it, and
// that the grouping follows the server's own change order rather than re-sorting.

function entry(
  name: string,
  change: DeckDiffEntry['change'],
  base: number,
  other: number,
  baseFoil = 0,
  otherFoil = 0,
): DeckDiffEntry {
  return {
    card: { id: `id-${name}`, name } as DeckDiffEntry['card'],
    name,
    change,
    base_quantity: base,
    other_quantity: other,
    delta: other - base,
    base_foil_quantity: baseFoil,
    other_foil_quantity: otherFoil,
  }
}

describe('deltaLabel', () => {
  it('signs a delta explicitly, with a real minus sign', () => {
    expect(deltaLabel(2)).toBe('+2')
    expect(deltaLabel(-1)).toBe('−1')
    expect(deltaLabel(0)).toBe('±0')
  })
})

describe('countsLabel / foilLabel', () => {
  it('prints both sides, with zero for a side that holds none', () => {
    expect(countsLabel(entry('Opt', 'removed', 4, 0))).toBe('4 → 0')
    expect(countsLabel(entry('Ponder', 'added', 0, 4))).toBe('0 → 4')
  })

  it('only mentions foils when a side holds one', () => {
    expect(foilLabel(entry('Sol Ring', 'changed', 1, 2))).toBe('')
    expect(foilLabel(entry('Sol Ring', 'finish', 1, 1, 1, 0))).toBe('1 foil → 0 foil')
  })
})

describe('summaryLabel', () => {
  it('lists only the non-zero kinds, pluralised', () => {
    expect(
      summaryLabel({ added: 3, removed: 1, changed: 2, finish_changed: 1, unchanged: 90 }),
    ).toBe('3 added · 1 removed · 2 count changes · 1 finish change')
    expect(
      summaryLabel({ added: 0, removed: 0, changed: 1, finish_changed: 0, unchanged: 99 }),
    ).toBe('1 count change')
  })

  it('says so when nothing differs', () => {
    expect(
      summaryLabel({ added: 0, removed: 0, changed: 0, finish_changed: 0, unchanged: 100 }),
    ).toBe('No differences')
  })
})

describe('groupByChange', () => {
  it('groups in the server order and drops empty kinds', () => {
    const groups = groupByChange([
      entry('Ponder', 'added', 0, 4),
      entry('Opt', 'removed', 4, 0),
      entry('Sol Ring', 'finish', 1, 1, 1, 0),
    ])
    expect(groups.map((g) => g.change)).toEqual(['added', 'removed', 'finish'])
    expect(groups.map((g) => g.entries.map((e) => e.name))).toEqual([
      ['Ponder'],
      ['Opt'],
      ['Sol Ring'],
    ])
    expect(DECK_DIFF_CHANGES).toEqual(['added', 'removed', 'changed', 'finish'])
  })
})
