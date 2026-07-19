import type { CardSet } from './api'

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

/** Case-insensitive substring match of `query` against a set's name or code. */
function setMatchesQuery(set: CardSet, query: string): boolean {
  return set.name.toLowerCase().includes(query) || set.code.toLowerCase().includes(query)
}

/**
 * Narrow pre-built {@link SetGroup}s to those where the main set **or any of its
 * related sub-sets** matches `query` (case-insensitive substring over name/code;
 * an empty/whitespace query keeps every group, returning the same array).
 *
 * A matched group is kept **whole**: matching only a sub-set (e.g. searching
 * "Jurassic World", a related set of Ixalan) still surfaces the entire group — the
 * main set and all its related sub-sets — rather than orphaning the matching tile
 * (issue #128). Grouping the full list with {@link groupSets} *before* filtering
 * is what makes this possible: filtering the flat list first would strand a
 * matched sub-set as a standalone root once its unmatched parent dropped out.
 */
export function filterGroups(groups: SetGroup[], query: string): SetGroup[] {
  const q = query.trim().toLowerCase()
  if (!q) return groups
  return groups.filter(
    (group) =>
      setMatchesQuery(group.main, q) || group.children.some((child) => setMatchesQuery(child, q)),
  )
}

/**
 * Whether `query` matches one of the group's **related sub-sets** (case-insensitive
 * substring over name/code; a blank/whitespace query matches nothing). This is the
 * signal for auto-revealing a collapsed related-sets dropdown: when {@link filterGroups}
 * keeps a group in the listing on the strength of a hidden sub-set, the matching tile
 * would otherwise stay tucked behind the collapsed toggle (issue #149). Independent of
 * whether the main set also matches — either way the sub-set is worth surfacing.
 */
export function queryMatchesRelated(group: SetGroup, query: string): boolean {
  const q = query.trim().toLowerCase()
  if (!q) return false
  return group.children.some((child) => setMatchesQuery(child, q))
}

/**
 * Find the group a given set code belongs to — whether `code` is the main set or
 * one of its sub-sets. Returns `undefined` if the code isn't in `sets`. Used by
 * the set page to offer an "include related sets" view rooted at the main set.
 */
export function findGroup(sets: CardSet[], code: string): SetGroup | undefined {
  return groupSets(sets).find((group) => groupHasCode(group, code))
}

/** Whether `code` is the main set or one of the sub-sets of `group`. */
export function groupHasCode(group: SetGroup, code: string): boolean {
  return group.main.code === code || group.children.some((c) => c.code === code)
}

/**
 * A sub-set's display label with its parent's redundant name prefix stripped
 * (e.g. "Bloomburrow Commander" → "Commander" under the "Bloomburrow" group).
 * Falls back to the full `name` when there's no shared prefix or nothing would be
 * left, so the label is never empty or misleading.
 */
export function subSetLabel(mainName: string, name: string): string {
  if (name.length > mainName.length && name.startsWith(mainName)) {
    const rest = name.slice(mainName.length).replace(/^[\s:–-]+/, '')
    if (rest) return rest
  }
  return name
}

/**
 * Which single set "View just this set" should drop back to when leaving the
 * grouped view: the set the grouped view was entered from (`from`) when it's
 * still a member of `group`, otherwise the group's main set. Validating
 * membership guards against a stale or hand-edited `from` in the URL.
 */
export function originSetCode(group: SetGroup, from: string | null | undefined): string {
  return from && groupHasCode(group, from) ? from : group.main.code
}

/**
 * Set codes pinned to the very top of the listing, ahead of the date-sorted
 * sections, in this order. Secret Lair Drop (`sld`) is an ongoing,
 * continuously-restocked product whose fixed 2019 release date would otherwise
 * bury it deep in the catalog, so we surface it first — it's a special case, not
 * a normal dated set.
 */
export const PINNED_SET_CODES = ['sld']

/**
 * Split any set-keyed items into the pinned ones (in {@link PINNED_SET_CODES}
 * order) and the rest (incoming order preserved), keying each item by
 * `codeOf`. A pinned code with no matching item is omitted, so this is a no-op
 * for a listing that has none of the pinned sets. The generic seam behind both
 * the card landing's {@link partitionPinned} (over {@link SetGroup}s) and the
 * sealed landing's per-set-tile pinning (over product-set refs) — one source of
 * truth for which sets lead the listing.
 */
export function partitionPinnedBy<T>(
  items: T[],
  codeOf: (item: T) => string,
): { pinned: T[]; rest: T[] } {
  const pinnedCodes = new Set(PINNED_SET_CODES)
  const byCode = new Map(items.map((item) => [codeOf(item), item]))
  const pinned = PINNED_SET_CODES.map((code) => byCode.get(code)).filter(
    (item): item is T => item !== undefined,
  )
  const rest = items.filter((item) => !pinnedCodes.has(codeOf(item)))
  return { pinned, rest }
}

/**
 * Split pre-built {@link SetGroup}s into the pinned groups (in
 * {@link PINNED_SET_CODES} order) and the rest (incoming order preserved). A
 * pinned code with no matching group is omitted, so this is a no-op for a game
 * that has none of the pinned sets.
 */
export function partitionPinned(groups: SetGroup[]): { pinned: SetGroup[]; rest: SetGroup[] } {
  return partitionPinnedBy(groups, (group) => group.main.code)
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
