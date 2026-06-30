import type { CardSet } from './api'

/**
 * Narrow a flat set list to those whose **name or code** contains `query`
 * (case-insensitive substring; an empty/whitespace query matches everything).
 *
 * Filtering the flat list *before* {@link groupSets} keeps the result honest:
 * every surviving tile actually matches, and a matching sub-set whose parent was
 * filtered out simply surfaces as its own top-level tile (an orphan becomes its
 * own root in {@link groupSets}).
 */
export function filterSets(sets: CardSet[], query: string): CardSet[] {
  const q = query.trim().toLowerCase()
  if (!q) return sets
  return sets.filter(
    (set) => set.name.toLowerCase().includes(q) || set.code.toLowerCase().includes(q),
  )
}

/** A main (top-level) set together with the sub-sets that hang off it. */
export interface SetGroup {
  main: CardSet
  /** Tokens, promos, Commander decks, art series, … sorted for display. */
  children: CardSet[]
}

// Display order for a sub-set within its group: the most "playable" supplements
// (Commander, Jumpstart) first, ephemera (tokens, art series) last. Unknown
// types land in the middle.
const CHILD_RANK: Record<string, number> = {
  expansion: 0,
  core: 0,
  commander: 1,
  draft_innovation: 2,
  masters: 3,
  masterpiece: 3,
  arsenal: 3,
  duel_deck: 4,
  from_the_vault: 4,
  premium_deck: 4,
  spellbook: 4,
  box: 4,
  starter: 4,
  planechase: 5,
  archenemy: 5,
  vanguard: 5,
  promo: 6,
  token: 7,
  memorabilia: 8,
  funny: 9,
  minigame: 9,
}

const childRank = (set: CardSet): number => CHILD_RANK[set.set_type ?? ''] ?? 5

function compareChildren(a: CardSet, b: CardSet): number {
  const byType = childRank(a) - childRank(b)
  if (byType !== 0) return byType
  // Newest first, then alphabetical for a stable order.
  const da = a.released_at ?? ''
  const db = b.released_at ?? ''
  if (da !== db) return da < db ? 1 : -1
  return a.name.localeCompare(b.name)
}

/**
 * Group a flat list of sets so that sub-sets (tokens, promos, Commander decks,
 * art series, …) are nested under the main set they belong to.
 *
 * Scryfall links a sub-set to its parent via `parent_set_code`, and that chain
 * can be two deep (e.g. *Bloomburrow Commander Tokens* → *Bloomburrow Commander*
 * → *Bloomburrow*). We resolve each set to its top-level **root** so every
 * sub-set lands in exactly one group, flattened to a single level. A set whose
 * `parent_set_code` points at a set we don't have (an orphan) is treated as its
 * own root and surfaced as a top-level set.
 *
 * The returned groups preserve the input order of the **main** sets (the caller
 * sorts newest-first); children are sorted by {@link compareChildren}.
 */
export function groupSets(sets: CardSet[]): SetGroup[] {
  const byCode = new Map(sets.map((set) => [set.code, set]))
  const position = new Map(sets.map((set, index) => [set.code, index]))

  // Walk parent links to the top-level root, guarding against missing parents
  // and (defensively) cycles in the data.
  const rootOf = (set: CardSet): CardSet => {
    let current = set
    const seen = new Set<string>()
    while (
      current.parent_set_code &&
      byCode.has(current.parent_set_code) &&
      !seen.has(current.code)
    ) {
      seen.add(current.code)
      current = byCode.get(current.parent_set_code)!
    }
    return current
  }

  const groups = new Map<string, SetGroup>()
  const ensure = (main: CardSet): SetGroup => {
    let group = groups.get(main.code)
    if (!group) {
      group = { main, children: [] }
      groups.set(main.code, group)
    }
    return group
  }

  for (const set of sets) {
    const root = rootOf(set)
    const group = ensure(root)
    if (root.code !== set.code) group.children.push(set)
  }

  for (const group of groups.values()) group.children.sort(compareChildren)

  // Order groups by each main set's own position in the input (newest-first),
  // not by when its group was first created — a child can appear before its
  // parent in the list.
  return [...groups.values()].sort(
    (a, b) => (position.get(a.main.code) ?? 0) - (position.get(b.main.code) ?? 0),
  )
}

/**
 * Find the group a given set code belongs to — whether `code` is the main set or
 * one of its sub-sets. Returns `undefined` if the code isn't in `sets`. Used by
 * the set page to offer an "include related sets" view rooted at the main set.
 */
export function findGroup(sets: CardSet[], code: string): SetGroup | undefined {
  return groupSets(sets).find(
    (group) => group.main.code === code || group.children.some((c) => c.code === code),
  )
}

/** A run of top-level set groups that share a release year. */
export interface SetYear {
  /** Release year, or `null` for sets with no/unparseable release date. */
  year: number | null
  groups: SetGroup[]
}

/**
 * Partition pre-built {@link SetGroup}s into release-year sections, newest year
 * first, with undated sets last. A group's year comes from its **main** set's
 * `released_at` (the sub-sets follow their parent into the same section), so the
 * within-year order produced by {@link groupSets} is preserved inside a section.
 */
export function groupByYear(groups: SetGroup[]): SetYear[] {
  const sections = new Map<number | null, SetGroup[]>()
  for (const group of groups) {
    const year = releaseYear(group.main)
    const bucket = sections.get(year)
    if (bucket) bucket.push(group)
    else sections.set(year, [group])
  }

  return [...sections.entries()]
    .map(([year, groups]) => ({ year, groups }))
    .sort((a, b) => {
      // Newest year first; undated (null) sinks to the bottom.
      if (a.year === b.year) return 0
      if (a.year === null) return 1
      if (b.year === null) return -1
      return b.year - a.year
    })
}

// Pull the year out of Scryfall's ISO `YYYY-MM-DD` date by slicing the leading
// four digits — parsing to a Date would risk a timezone shift across New Year.
function releaseYear(set: CardSet): number | null {
  if (!set.released_at) return null
  const year = Number.parseInt(set.released_at.slice(0, 4), 10)
  return Number.isNaN(year) ? null : year
}
