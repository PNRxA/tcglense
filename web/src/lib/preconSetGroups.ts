import type { CardSet, PreconSetRef } from './api'
import { rootSetCode } from './setGroups'

/**
 * The preconstructed-deck landing's set grouping: the sets that published precons, nested so a
 * set's related sub-sets sit under the main set they belong to — the deck mirror of the card
 * catalog landing's {@link import('./setGroups').SetGroup}.
 *
 * Same parent chain, resolved through the same {@link rootSetCode} seam, but over a different
 * list: the landing only lists sets that *have* precons, where the card landing lists every
 * set. Two consequences fall out of that:
 *
 * - The root is resolved against the **catalog**, not against the precon sets, or a Commander
 *   sub-set whose parent published no precons would look like its own root.
 * - A group's `main` is normally the root itself (for MTG today, every group's root does
 *   publish precons), but it *can* be absent — a set could ship only a Commander sub-set. The
 *   first member in input order (the facets endpoint sorts newest-first) then leads, so a
 *   group always has a real, clickable main rather than a heading with nothing behind it.
 */
export interface PreconSetGroup {
  main: PreconSetRef
  /** The group's other precon-publishing sets, most decks first. */
  children: PreconSetRef[]
}

/**
 * Nest `sets` (the precon facets' set list) under their catalog roots.
 *
 * `catalogSets` is the game's full set list — the parent links live there, not on the facet
 * rows. A set with no catalog row is its own root, so it degrades to a standalone tile rather
 * than disappearing. Group order follows the input order of each group's `main`, so the
 * caller's sort (newest first) is preserved.
 */
export function groupPreconSets(sets: PreconSetRef[], catalogSets: CardSet[]): PreconSetGroup[] {
  const byCode = new Map(catalogSets.map((set) => [set.code, set]))
  const members = new Map<string, PreconSetRef[]>()
  for (const set of sets) {
    const root = rootSetCode(set.code, byCode)
    const bucket = members.get(root)
    if (bucket) bucket.push(set)
    else members.set(root, [set])
  }

  const groups: PreconSetGroup[] = []
  for (const [root, bucket] of members) {
    // The root itself when it publishes precons, else the first member (input order).
    const mainIndex = Math.max(
      bucket.findIndex((set) => set.code === root),
      0,
    )
    const main = bucket[mainIndex]
    // Unreachable: a bucket exists only because a set was pushed into it.
    if (!main) continue
    bucket.splice(mainIndex, 1)
    // Most decks first: on this landing, how many decks a sub-set holds is what distinguishes
    // it (the card landing ranks its children by set *type* instead, which is what
    // distinguishes a token set from a Commander set there).
    bucket.sort((a, b) => b.count - a.count || a.code.localeCompare(b.code))
    groups.push({ main, children: bucket })
  }
  return groups
}

/** Case-insensitive substring match of `needle` against a set's name or code. */
function matches(set: PreconSetRef, needle: string): boolean {
  return !!set.name?.toLowerCase().includes(needle) || set.code.toLowerCase().includes(needle)
}

/**
 * Whether one of a group's **related** sub-sets matches `needle` — the landing's filter uses it
 * both to keep a group whole when only a child matched and to auto-open that group's dropdown,
 * so the match isn't hidden behind the collapsed toggle (the card landing's issue #149 rule).
 *
 * `needle` is expected already trimmed + lower-cased; an empty one matches nothing.
 */
export function preconGroupMatchesRelated(group: PreconSetGroup, needle: string): boolean {
  if (!needle) return false
  return group.children.some((set) => matches(set, needle))
}

/** Whether a group matches at all — its main set or any related sub-set. */
export function preconGroupMatches(group: PreconSetGroup, needle: string): boolean {
  if (!needle) return true
  return matches(group.main, needle) || preconGroupMatchesRelated(group, needle)
}
