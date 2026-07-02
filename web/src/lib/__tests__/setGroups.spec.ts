import { describe, it, expect } from 'vitest'

import {
  filterGroups,
  findGroup,
  groupByYear,
  groupHasCode,
  groupSets,
  originSetCode,
  partitionPinned,
  PINNED_SET_CODES,
  queryMatchesRelated,
  subSetLabel,
} from '../setGroups'
import { makeCardSet } from '@/test/fixtures'

const set = makeCardSet

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

describe('groupHasCode', () => {
  const group = {
    main: set('blb'),
    children: [set('blc', { parent_set_code: 'blb' }), set('tblb', { parent_set_code: 'blb' })],
  }

  it('matches the main set code', () => {
    expect(groupHasCode(group, 'blb')).toBe(true)
  })

  it('matches a sub-set code', () => {
    expect(groupHasCode(group, 'tblb')).toBe(true)
  })

  it('rejects a code outside the group', () => {
    expect(groupHasCode(group, 'neo')).toBe(false)
  })
})

describe('subSetLabel', () => {
  it('strips the redundant parent prefix from a sub-set name', () => {
    expect(subSetLabel('Bloomburrow', 'Bloomburrow Commander')).toBe('Commander')
  })

  it('trims separators left after the prefix', () => {
    expect(subSetLabel('Bloomburrow', 'Bloomburrow: Tokens')).toBe('Tokens')
    expect(subSetLabel('Bloomburrow', 'Bloomburrow – Promos')).toBe('Promos')
  })

  it('falls back to the full name when there is no shared prefix', () => {
    expect(subSetLabel('Bloomburrow', 'Wilds of Eldraine')).toBe('Wilds of Eldraine')
  })

  it('falls back to the full name when nothing would be left after the prefix', () => {
    expect(subSetLabel('Bloomburrow', 'Bloomburrow')).toBe('Bloomburrow')
  })
})

describe('originSetCode', () => {
  const group = {
    main: set('blb'),
    children: [set('blc', { parent_set_code: 'blb' }), set('tblb', { parent_set_code: 'blb' })],
  }

  it('returns the entered-from sub-set when it is a member of the group', () => {
    // The bug fix: a sub-set → "view all together" → "view just this set" lands
    // back on the original sub-set, not the parent.
    expect(originSetCode(group, 'blc')).toBe('blc')
  })

  it('returns the main set when entered from the main set', () => {
    expect(originSetCode(group, 'blb')).toBe('blb')
  })

  it('falls back to the main set when there is no from', () => {
    expect(originSetCode(group, null)).toBe('blb')
    expect(originSetCode(group, undefined)).toBe('blb')
  })

  it('ignores a stale or bogus from that is not in the group', () => {
    expect(originSetCode(group, 'neo')).toBe('blb')
  })
})

describe('filterGroups', () => {
  // Ixalan with "Jurassic World" (a related Universes Beyond set) nested under it,
  // plus that set's own tokens two levels deep — the shape from issue #128.
  const groups = groupSets([
    set('xln', { name: 'Ixalan' }),
    set('rex', {
      name: 'Jurassic World Collection',
      parent_set_code: 'xln',
      set_type: 'commander',
    }),
    set('trex', { name: 'Jurassic World Tokens', parent_set_code: 'rex', set_type: 'token' }),
    set('neo', { name: 'Kamigawa: Neon Dynasty' }),
  ])

  it('returns the original groups for an empty or whitespace query', () => {
    expect(filterGroups(groups, '')).toBe(groups)
    expect(filterGroups(groups, '   ')).toBe(groups)
  })

  it('keeps a whole group when a related sub-set matches (issue #128)', () => {
    // "Jurassic" matches only the related sub-sets, not Ixalan itself — but the
    // entire Ixalan group (main + every related sub-set) must surface, not just
    // the matching tiles.
    const result = filterGroups(groups, 'jurassic')
    expect(result.map((g) => g.main.code)).toEqual(['xln'])
    expect(result[0]?.children.map((c) => c.code)).toEqual(['rex', 'trex'])
  })

  it('keeps a whole group when the main set matches', () => {
    const result = filterGroups(groups, 'ixalan')
    expect(result.map((g) => g.main.code)).toEqual(['xln'])
    expect(result[0]?.children.map((c) => c.code)).toEqual(['rex', 'trex'])
  })

  it('matches a set code case-insensitively, including a sub-set code', () => {
    expect(filterGroups(groups, 'NEO').map((g) => g.main.code)).toEqual(['neo'])
    // 'trex' is a two-levels-deep sub-set of Ixalan; matching it surfaces xln.
    expect(filterGroups(groups, 'trex').map((g) => g.main.code)).toEqual(['xln'])
  })

  it('trims surrounding whitespace from the query', () => {
    expect(filterGroups(groups, '  neon  ').map((g) => g.main.code)).toEqual(['neo'])
  })

  it('returns an empty list when nothing matches', () => {
    expect(filterGroups(groups, 'zzz')).toEqual([])
  })

  it('excludes a group when the match belongs only to a different group', () => {
    const result = filterGroups(groups, 'neon')
    expect(result.map((g) => g.main.code)).toEqual(['neo'])
    expect(result[0]?.children).toEqual([])
  })
})

describe('queryMatchesRelated', () => {
  // Ixalan with a "Jurassic World" sub-set (and its tokens) nested under it — the same
  // shape as the filterGroups tests, so a query can hit a sub-set but not the main.
  const group = {
    main: set('xln', { name: 'Ixalan' }),
    children: [
      set('rex', { name: 'Jurassic World Collection', parent_set_code: 'xln' }),
      set('trex', { name: 'Jurassic World Tokens', parent_set_code: 'xln' }),
    ],
  }
  const childless = { main: set('neo', { name: 'Kamigawa: Neon Dynasty' }), children: [] }

  it('is true when the query matches a related sub-set by name or code', () => {
    expect(queryMatchesRelated(group, 'jurassic')).toBe(true)
    expect(queryMatchesRelated(group, 'TREX')).toBe(true)
  })

  it('is false when only the main set matches (name or code)', () => {
    expect(queryMatchesRelated(group, 'ixalan')).toBe(false)
    expect(queryMatchesRelated(group, 'xln')).toBe(false)
  })

  it('is true when the main set and a sub-set both match — a sub-set hit is enough', () => {
    const blb = {
      main: set('blb', { name: 'Bloomburrow' }),
      children: [set('blc', { name: 'Bloomburrow Commander', parent_set_code: 'blb' })],
    }
    expect(queryMatchesRelated(blb, 'bloomburrow')).toBe(true)
  })

  it('is false for a blank/whitespace query', () => {
    expect(queryMatchesRelated(group, '')).toBe(false)
    expect(queryMatchesRelated(group, '   ')).toBe(false)
  })

  it('trims surrounding whitespace before matching', () => {
    expect(queryMatchesRelated(group, '  jurassic  ')).toBe(true)
  })

  it('is false for a childless group and when nothing matches', () => {
    expect(queryMatchesRelated(childless, 'neon')).toBe(false)
    expect(queryMatchesRelated(group, 'zzz')).toBe(false)
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
