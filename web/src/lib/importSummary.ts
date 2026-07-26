import type { ImportSummary } from '@/lib/api'

/**
 * Human-readable summary lines for a completed collection import/sync, for the import
 * dialog's result panel. Kept as a pure function (no Vue) so it's unit-testable and could
 * be reused by any other import-outcome surface. The collection landing shows its own
 * one-line re-sync message instead, so it doesn't use this.
 *
 * Lead line: how many cards were imported/updated (and total copies). Then, for smart
 * sync, whether it stopped early; then any unmatched (skipped) cards; then any removed to
 * mirror the list. Counts are localized; the leading verb switches to "Updated" for smart.
 */
export function formatImportSummaryLines(summary: ImportSummary): string[] {
  const lines: string[] = []
  const copies = summary.regular_copies + summary.foil_copies
  const verb = summary.mode === 'smart' ? 'Updated' : 'Imported'
  lines.push(
    `${verb} ${summary.matched_cards.toLocaleString()} card${summary.matched_cards === 1 ? '' : 's'} ` +
      `(${copies.toLocaleString()} cop${copies === 1 ? 'y' : 'ies'}).`,
  )
  if (summary.mode === 'smart') {
    lines.push(
      summary.stopped_early
        ? 'Smart sync stopped once it reached cards already in sync.'
        : 'Smart sync scanned your whole collection.',
    )
  }
  if (summary.unmatched_cards > 0) {
    lines.push(
      summary.unmatched_cards === 1
        ? '1 card wasn’t in our catalog and was skipped.'
        : `${summary.unmatched_cards.toLocaleString()} cards weren’t in our catalog and were skipped.`,
    )
  }
  if (summary.removed_cards > 0) {
    lines.push(
      `${summary.removed_cards.toLocaleString()} card${summary.removed_cards === 1 ? '' : 's'} ` +
        'removed to mirror the list.',
    )
  }
  return lines
}
