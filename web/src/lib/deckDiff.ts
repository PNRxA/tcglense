import type { DeckDiffChange, DeckDiffEntry, DeckDiffSummary } from '@/lib/api'

// Wording for the deck diff (issue #674). The fold itself is the server's
// (`GET /api/decks/{game}/{deck_id}/diff/{other_id}`, folded by card name across printings
// and finishes); this module only turns its numbers into the short labels the compare panel
// prints, kept out of the component so the exact strings can be pinned.

/** The kinds of change in the order the server lists them — and the order the panel groups
 * them in, so a section's rows and its legend can't disagree. */
export const DECK_DIFF_CHANGES: readonly DeckDiffChange[] = [
  'added',
  'removed',
  'changed',
  'finish',
]

/** A short heading per kind of change. */
export const DECK_DIFF_CHANGE_LABEL: Record<DeckDiffChange, string> = {
  added: 'Added',
  removed: 'Removed',
  changed: 'Count changed',
  finish: 'Finish changed',
}

/** The chip classes per kind — the design system's semantic tokens in the `bg-<token>/15
 * text-<token>` chip idiom, never a palette class. Spelled out as literals (not built from a
 * token name at runtime) so Tailwind's source scan sees every class it must emit. */
export const DECK_DIFF_CHANGE_CLASS: Record<DeckDiffChange, string> = {
  added: 'bg-success/15 text-success',
  removed: 'bg-destructive/15 text-destructive',
  changed: 'bg-warning/15 text-warning',
  finish: 'bg-info/15 text-info',
}

/** `+2` / `−1` / `±0`: the copy delta with an explicit sign, using a real minus sign so a
 * removal doesn't read as a hyphenated word. */
export function deltaLabel(delta: number): string {
  if (delta > 0) return `+${delta}`
  if (delta < 0) return `−${Math.abs(delta)}`
  return '±0'
}

/** `2 → 4`: the copies on each side. For an added or removed card one side is `0`, which is
 * the honest reading (the deck held none) rather than a blank. */
export function countsLabel(entry: DeckDiffEntry): string {
  return `${entry.base_quantity} → ${entry.other_quantity}`
}

/** The foil side of an entry — `1 foil → 0 foil` — or `''` when neither deck holds a foil
 * copy, so the common all-regular row carries no finish noise. */
export function foilLabel(entry: DeckDiffEntry): string {
  if (entry.base_foil_quantity === 0 && entry.other_foil_quantity === 0) return ''
  return `${entry.base_foil_quantity} foil → ${entry.other_foil_quantity} foil`
}

/** One sentence for the panel header: `3 added · 1 removed · 2 count changes · 1 finish
 * change`, listing only the non-zero kinds, or `No differences` when every card matches. */
export function summaryLabel(summary: DeckDiffSummary): string {
  const parts: string[] = []
  if (summary.added > 0) parts.push(`${summary.added} added`)
  if (summary.removed > 0) parts.push(`${summary.removed} removed`)
  if (summary.changed > 0) {
    parts.push(`${summary.changed} count change${summary.changed === 1 ? '' : 's'}`)
  }
  if (summary.finish_changed > 0) {
    parts.push(`${summary.finish_changed} finish change${summary.finish_changed === 1 ? '' : 's'}`)
  }
  return parts.length ? parts.join(' · ') : 'No differences'
}

/** Group a section's (already sorted) entries by kind, in `DECK_DIFF_CHANGES` order, dropping
 * the empty kinds — what the panel renders as sub-lists under one section heading. */
export function groupByChange(
  entries: readonly DeckDiffEntry[],
): Array<{ change: DeckDiffChange; entries: DeckDiffEntry[] }> {
  return DECK_DIFF_CHANGES.map((change) => ({
    change,
    entries: entries.filter((entry) => entry.change === change),
  })).filter((group) => group.entries.length > 0)
}
