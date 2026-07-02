// Shared formatters for the collection's owned-count labels, so the per-set tiles
// (SetTile) and the browse header (CollectionBrowseView) render them identically — both
// localized, where SetTile previously wasn't.

/** The word a completion count ends with: `owned` for the collection (the default
 * everywhere), `wanted` for the wish list's tiles and browse headers (issue #167). */
export type CountNoun = 'owned' | 'wanted'

/**
 * A slash-form "X/Y owned" set-completion label (or "X/Y wanted" with the wish list's
 * noun). `owned` is clamped to `total` so a paper-only vs. Scryfall card-count skew can
 * never read "N+1 of N". Both counts are localized (thousands separators).
 */
export function formatCompletion(owned: number, total: number, noun: CountNoun = 'owned'): string {
  return `${Math.min(owned, total).toLocaleString()}/${total.toLocaleString()} ${noun}`
}

/** A "N copies" label for total owned copies (regular + foil, i.e. counting duplicates). */
export function formatCopies(copies: number): string {
  return `${copies.toLocaleString()} copies`
}
