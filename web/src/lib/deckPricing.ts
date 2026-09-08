import type { Card, DeckPricingLine } from '@/lib/api'

// The small pure half of the deck pricing panel (issue #672): what counts as a swappable
// row, how a printing is named in a table cell, and how many rows the resting state shows.
// Kept out of the component so the spec can pin them without mounting it, and so the
// "swap all" batch and the per-row button can't disagree about which rows qualify.

/** How many of the most expensive rows the collapsed panel names. */
export const TOP_EXPENSIVE = 5

/** Whether swapping this row to its cheapest printing would save money — the server stated
 * a saving (it only does when both sides are priced) and it is above zero. A row whose
 * saving is unknown is never offered a swap, however cheap its `cheapest` looks. */
export function hasSaving(line: DeckPricingLine): boolean {
  if (!line.cheapest || line.saving_usd == null) return false
  const cents = Math.round(Number(line.saving_usd) * 100)
  return Number.isFinite(cents) && cents > 0
}

/** `SET · #12` — the printing, as the table names it beside a card. */
export function printingLabel(card: Card): string {
  return `${card.set_code.toUpperCase()} · #${card.collector_number}`
}
