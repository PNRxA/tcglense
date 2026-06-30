import { describe, it, expect } from 'vitest'

import {
  filterSets,
  findGroup,
  groupByYear,
  groupSets,
  partitionPinned,
  PINNED_SET_CODES,
} from '../setGroups'
import type { CardSet } from '../api'

function set(code: string, over: Partial<CardSet> = {}): CardSet {
  return {
    code,
    name: code.toUpperCase(),
    set_type: 'expansion',
    released_at: '2024-01-01',
    card_count: 100,
    icon_svg_uri: null,
    parent_set_code: null,
    ...over,
  }
}

const mains = (groups: ReturnType<typeof groupSets>) => groups.map((g) => g.main.code)
const children = (groups: ReturnType<typeof groupSets>) =>
  groups.flatMap((g) => g.children.map((c) => c.code))

describe('groupSets', () => {
  it('keeps parentless sets as their own groups, preserving order', () => {
    const groups = groupSets([set('a'), set('b'), set('c')])
    expect(mains(groups)).toEqual(['a', 'b', 'c'])
    expect(children(groups)).toEqual([])
  })

  it('nests sub-sets under their parent', () => {
    const groups = groupSets([
      set('blb'),
      set('blc', { parent_set_code: 'blb', set_type: 'commander' }),
      set('tblb', { parent_set_code: 'blb', set_type: 'token' }),
    ])
    expect(mains(groups)).toEqual(['blb'])
    expect(children(groups)).toEqual(['blc', 'tblb'])
  })

  it('orders children by set-type relevance (commander > promo > token > art series)', () => {
    const groups = groupSets([
      set('p'),
      set('art', { parent_set_code: 'p', set_type: 'memorabilia' }),
      set('tok', { parent_set_code: 'p', set_type: 'token' }),
      set('cmd', { parent_set_code: 'p', set_type: 'commander' }),
      set('promo', { parent_set_code: 'p', set_type: 'promo' }),
    ])
    expect(children(groups)).toEqual(['cmd', 'promo', 'tok', 'art'])
  })

  it('flattens a two-level parent chain into the top-level root', () => {
    // tblc -> blc -> blb: the grandchild lands directly under blb.
    const groups = groupSets([
      set('blb'),
      set('blc', { parent_set_code: 'blb', set_type: 'commander' }),
      set('tblc', { parent_set_code: 'blc', set_type: 'token' }),
    ])
    expect(mains(groups)).toEqual(['blb'])
    expect(children(groups).sort()).toEqual(['blc', 'tblc'])
  })

  it('treats an orphan (parent not in the list) as its own top-level set', () => {
    const groups = groupSets([set('pmic', { parent_set_code: 'past', set_type: 'memorabilia' })])
    expect(mains(groups)).toEqual(['pmic'])
    expect(children(groups)).toEqual([])
  })

  it('positions a group by its main set, even when a child appears earlier', () => {
    // Input order: child of b, then b, then a. The group for b must sit at b's
    // position, not at the child's earlier position.
    const groups = groupSets([
      set('cb', { parent_set_code: 'b', set_type: 'token' }),
      set('b'),
      set('a'),
    ])
    expect(mains(groups)).toEqual(['b', 'a'])
  })

  it('does not loop forever on a cyclic parent reference', () => {
    const groups = groupSets([
      set('a', { parent_set_code: 'b' }),
      set('b', { parent_set_code: 'a' }),
    ])
    // Degenerate data: both resolve to themselves rather than hanging.
    expect(groups).toHaveLength(2)
  })
})

describe('findGroup', () => {
  const sets = [
    set('blb'),
    set('blc', { parent_set_code: 'blb', set_type: 'commander' }),
    set('tblc', { parent_set_code: 'blc', set_type: 'token' }),
    set('dft'),
  ]

  it('finds the group from the main set code', () => {
    const group = findGroup(sets, 'blb')
    expect(group?.main.code).toBe('blb')
    expect(group?.children.map((c) => c.code).sort()).toEqual(['blc', 'tblc'])
  })

  it('finds the same group from a sub-set code (root + all descendants)', () => {
    expect(findGroup(sets, 'blc')?.main.code).toBe('blb')
    expect(findGroup(sets, 'tblc')?.main.code).toBe('blb')
  })

  it('returns a standalone set as its own childless group', () => {
    const group = findGroup(sets, 'dft')
    expect(group?.main.code).toBe('dft')
    expect(group?.children).toEqual([])
  })

  it('returns undefined for a code not in the list', () => {
    expect(findGroup(sets, 'zzz')).toBeUndefined()
  })
})

describe('filterSets', () => {
  const sets = [
    set('blb', { name: 'Bloomburrow' }),
    set('blc', { name: 'Bloomburrow Commander', parent_set_code: 'blb' }),
    set('neo', { name: 'Kamigawa: Neon Dynasty' }),
  ]

  it('returns the original list for an empty or whitespace query', () => {
    expect(filterSets(sets, '')).toBe(sets)
    expect(filterSets(sets, '   ')).toBe(sets)
  })

  it('matches set names case-insensitively, as a substring', () => {
    expect(filterSets(sets, 'bloom').map((s) => s.code)).toEqual(['blb', 'blc'])
    expect(filterSets(sets, 'NEON').map((s) => s.code)).toEqual(['neo'])
  })

  it('matches the set code, case-insensitively', () => {
    expect(filterSets(sets, 'NEO').map((s) => s.code)).toEqual(['neo'])
    expect(filterSets(sets, 'blc').map((s) => s.code)).toEqual(['blc'])
  })

  it('trims surrounding whitespace from the query', () => {
    expect(filterSets(sets, '  neon  ').map((s) => s.code)).toEqual(['neo'])
  })

  it('returns an empty list when nothing matches', () => {
    expect(filterSets(sets, 'zzz')).toEqual([])
  })

  it('feeds groupSets so an orphaned matching child becomes its own top-level tile', () => {
    // 'commander' matches only the child; its parent is filtered out, so the
    // child surfaces as a top-level group rather than vanishing.
    const groups = groupSets(filterSets(sets, 'commander'))
    expect(groups.map((g) => g.main.code)).toEqual(['blc'])
    expect(groups.flatMap((g) => g.children)).toEqual([])
  })
})

describe('groupByYear', () => {
  it('splits groups into year sections, newest year first', () => {
    const sections = groupByYear(
      groupSets([
        set('a', { released_at: '2024-09-01' }),
        set('b', { released_at: '2023-05-01' }),
        set('c', { released_at: '2025-01-01' }),
      ]),
    )
    expect(sections.map((s) => s.year)).toEqual([2025, 2024, 2023])
    expect(sections.map((s) => s.groups.map((g) => g.main.code))).toEqual([['c'], ['a'], ['b']])
  })

  it('keeps several groups from the same year in their incoming order', () => {
    const sections = groupByYear(
      groupSets([
        set('a', { released_at: '2024-09-01' }),
        set('b', { released_at: '2024-03-01' }),
        set('c', { released_at: '2023-01-01' }),
      ]),
    )
    expect(
      sections.map((s) => ({ year: s.year, codes: s.groups.map((g) => g.main.code) })),
    ).toEqual([
      { year: 2024, codes: ['a', 'b'] },
      { year: 2023, codes: ['c'] },
    ])
  })

  it('buckets a child under its parent set year, not its own release date', () => {
    // A promo released in 2025 still belongs to its 2024 parent's section.
    const sections = groupByYear(
      groupSets([
        set('blb', { released_at: '2024-08-01' }),
        set('pblb', { parent_set_code: 'blb', set_type: 'promo', released_at: '2025-02-01' }),
      ]),
    )
    expect(sections.map((s) => s.year)).toEqual([2024])
    expect(sections.flatMap((s) => s.groups.flatMap((g) => g.children.map((c) => c.code)))).toEqual(
      ['pblb'],
    )
  })

  it('sinks undated sets into a trailing null section', () => {
    const sections = groupByYear(
      groupSets([
        set('a', { released_at: '2024-01-01' }),
        set('b', { released_at: null }),
        set('c', { released_at: '' }),
      ]),
    )
    expect(
      sections.map((s) => ({ year: s.year, codes: s.groups.map((g) => g.main.code) })),
    ).toEqual([
      { year: 2024, codes: ['a'] },
      { year: null, codes: ['b', 'c'] },
    ])
  })
})

describe('partitionPinned', () => {
  it('pulls a pinned set to the front and keeps the rest in order', () => {
    const { pinned, rest } = partitionPinned(groupSets([set('a'), set('sld'), set('b')]))
    expect(pinned.map((g) => g.main.code)).toEqual(['sld'])
    expect(rest.map((g) => g.main.code)).toEqual(['a', 'b'])
  })

  it('keeps a pinned set together with its nested children', () => {
    const { pinned, rest } = partitionPinned(
      groupSets([set('sld'), set('slp', { parent_set_code: 'sld', set_type: 'promo' }), set('a')]),
    )
    expect(pinned.map((g) => g.main.code)).toEqual(['sld'])
    expect(pinned[0]?.children.map((c) => c.code)).toEqual(['slp'])
    expect(rest.map((g) => g.main.code)).toEqual(['a'])
  })

  it('is a no-op when no pinned set is present', () => {
    const { pinned, rest } = partitionPinned(groupSets([set('a'), set('b')]))
    expect(pinned).toEqual([])
    expect(rest.map((g) => g.main.code)).toEqual(['a', 'b'])
  })

  // Regression guard for the "pinned follow PINNED_SET_CODES order, not input
  // order" contract. With a single pinned code today it can't yet distinguish the
  // two orderings, but it strengthens automatically once a second code is pinned
  // (the reversed input would then differ from PINNED_SET_CODES order).
  it('orders pinned groups by PINNED_SET_CODES regardless of input order', () => {
    const codes = [...PINNED_SET_CODES]
    const { pinned } = partitionPinned(groupSets(codes.map((c) => set(c)).reverse()))
    expect(pinned.map((g) => g.main.code)).toEqual(codes)
  })
})
