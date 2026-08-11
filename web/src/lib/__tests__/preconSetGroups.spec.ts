import { describe, expect, it } from 'vitest'
import type { CardSet, PreconSetRef } from '@/lib/api'
import {
  groupPreconSets,
  preconGroupMatches,
  preconGroupMatchesRelated,
} from '@/lib/preconSetGroups'

function catalogSet(code: string, name: string, parent?: string): CardSet {
  return { code, name, parent_set_code: parent ?? null } as CardSet
}
function preconSet(code: string, name: string, count: number): PreconSetRef {
  return { code, name, count, released_at: null }
}

// A set with a Commander sub-set and a token sub-set — the real shape (`blb` → `blc`/`tblb`),
// where only some sub-sets publish decks.
const CATALOG = [
  catalogSet('blb', 'Bloomburrow'),
  catalogSet('blc', 'Bloomburrow Commander', 'blb'),
  catalogSet('tblb', 'Bloomburrow Tokens', 'blb'),
  catalogSet('sld', 'Secret Lair Drop'),
  catalogSet('fdn', 'Foundations'),
  catalogSet('j25', 'Foundations Jumpstart', 'fdn'),
]

describe('groupPreconSets', () => {
  it('nests a set’s related sub-sets under the set they belong to', () => {
    const groups = groupPreconSets(
      [preconSet('blb', 'Bloomburrow', 2), preconSet('blc', 'Bloomburrow Commander', 4)],
      CATALOG,
    )
    expect(groups).toHaveLength(1)
    expect(groups[0]?.main.code).toBe('blb')
    expect(groups[0]?.children.map((set) => set.code)).toEqual(['blc'])
  })

  it('keeps a set with no precon-publishing siblings standalone', () => {
    const groups = groupPreconSets([preconSet('sld', 'Secret Lair Drop', 700)], CATALOG)
    expect(groups).toHaveLength(1)
    expect(groups[0]?.children).toEqual([])
  })

  it('resolves the root against the catalog, not the precon list', () => {
    // `blb` publishes no decks itself, so the group is led by the sub-set that does — a
    // heading with nothing behind it would be unclickable.
    const groups = groupPreconSets([preconSet('blc', 'Bloomburrow Commander', 4)], CATALOG)
    expect(groups).toHaveLength(1)
    expect(groups[0]?.main.code).toBe('blc')
    expect(groups[0]?.children).toEqual([])
  })

  it('orders children by deck count, and groups by their main’s input order', () => {
    const groups = groupPreconSets(
      [
        preconSet('fdn', 'Foundations', 5),
        preconSet('blb', 'Bloomburrow', 2),
        preconSet('j25', 'Foundations Jumpstart', 128),
        preconSet('blc', 'Bloomburrow Commander', 4),
      ],
      CATALOG,
    )
    // Input order of the mains is preserved (the facets endpoint sorts newest-first).
    expect(groups.map((group) => group.main.code)).toEqual(['fdn', 'blb'])
    // A sub-set with far more decks leads its group's children.
    expect(groups[0]?.children.map((set) => set.code)).toEqual(['j25'])
  })

  it('treats a set with no catalog row as its own root', () => {
    const groups = groupPreconSets([preconSet('zzz', 'Unknown Set', 1)], CATALOG)
    expect(groups).toHaveLength(1)
    expect(groups[0]?.main.code).toBe('zzz')
  })

  it('terminates on a parent cycle rather than hanging', () => {
    const cyclic = [catalogSet('a', 'A', 'b'), catalogSet('b', 'B', 'a')]
    const groups = groupPreconSets([preconSet('a', 'A', 1), preconSet('b', 'B', 1)], cyclic)
    expect(groups.reduce((n, group) => n + 1 + group.children.length, 0)).toBe(2)
  })
})

describe('preconGroupMatches', () => {
  const group = groupPreconSets(
    [preconSet('blb', 'Bloomburrow', 2), preconSet('blc', 'Bloomburrow Commander', 4)],
    CATALOG,
  )[0]!

  it('keeps a group whole when only a related sub-set matches', () => {
    // Searching the sub-set's distinguishing word must not orphan it from its group.
    expect(preconGroupMatches(group, 'commander')).toBe(true)
    expect(preconGroupMatchesRelated(group, 'commander')).toBe(true)
  })

  it('matches on the main set by name or code, without flagging it as a related match', () => {
    expect(preconGroupMatches(group, 'bloomburrow')).toBe(true)
    expect(preconGroupMatches(group, 'blb')).toBe(true)
    // 'blb' is the main's own code, so the dropdown shouldn't auto-open for it.
    expect(preconGroupMatchesRelated(group, 'blb')).toBe(false)
  })

  it('keeps every group for an empty needle, and drops a non-match', () => {
    expect(preconGroupMatches(group, '')).toBe(true)
    expect(preconGroupMatchesRelated(group, '')).toBe(false)
    expect(preconGroupMatches(group, 'ixalan')).toBe(false)
  })
})
