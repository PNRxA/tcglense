import type { CollectionQuantities, NeededCard, NeededTotals } from '@/lib/api'

// The wording of a shopping list's size and cost (issue #675), stated once so the deck
// header's "to finish" line and the needed view's summary can't disagree on what the same
// `totals` say. Money arrives as canonical USD strings; the caller hands in its display
// formatter (the currency store's `formatUsd`), which returns `null` for an unpriced value.

/** "12 cards · 15 copies · ~$41.20 at your printings · from $28.10 at cheapest printings
 *  · 2 unpriced". Each clause is present only when it says something: the copies clause
 *  when copies outnumber cards, the cheapest clause when it differs from the held price,
 *  the unpriced clause whenever a total is a floor — so a fully priced list never carries
 *  a "0 unpriced" and a list with nothing priced names no money at all. */
export function neededCostLine(
  totals: NeededTotals,
  formatUsd: (raw: string | null | undefined) => string | null,
): string {
  const parts: string[] = [plural(totals.cards, 'card')]
  if (totals.copies !== totals.cards) parts.push(`${plural(totals.copies, 'copy', 'copies')}`)
  const held = formatUsd(totals.held_usd)
  const cheapest = formatUsd(totals.cheapest_usd)
  if (held) parts.push(`~${held} at your printings`)
  if (cheapest && cheapest !== held) parts.push(`from ${cheapest} at cheapest printings`)
  // Either column may be a floor; the larger count is the honest caveat for the line.
  const unpriced = Math.max(totals.held_unpriced_cards, totals.cheapest_unpriced_cards)
  if (unpriced > 0) parts.push(`${unpriced} unpriced`)
  return parts.join(' · ')
}

/** "$3.50 · from $1.20" for one entry — the held price, then the cheapest when it is
 *  lower; `null` when the entry is priced neither way. */
export function neededEntryPrice(
  entry: Pick<NeededCard, 'held_usd' | 'cheapest_usd'>,
  formatUsd: (raw: string | null | undefined) => string | null,
): string | null {
  const held = formatUsd(entry.held_usd)
  const cheapest = formatUsd(entry.cheapest_usd)
  if (held && cheapest && cheapest !== held) return `${held} · from ${cheapest}`
  return held ?? cheapest
}

/** One wish-list write that "add all to wish list" would make: the card, and the counts
 *  that leave the wish list holding at least the copies still needed. */
export interface WishlistTopUp {
  id: string
  quantity: number
  foil_quantity: number
}

/**
 * Plan the writes that put every needed card on the wish list — **topping up, never
 * adding**: a card already wanted in at least `needed` copies (either finish) is left
 * alone, and one wanted in fewer has its *regular* count raised until the total reaches
 * `needed`, its foil count untouched. So the action is idempotent (a second click plans
 * nothing) and never lowers a count the user set by hand. Entries whose current counts are
 * unknown (`wanted` has no row) are read as not wanted at all.
 */
export function planWishlistTopUps(
  entries: Pick<NeededCard, 'card' | 'needed'>[],
  wanted: Record<string, CollectionQuantities | undefined>,
): WishlistTopUp[] {
  const plan: WishlistTopUp[] = []
  for (const entry of entries) {
    const have = wanted[entry.card.id] ?? { quantity: 0, foil_quantity: 0 }
    const short = entry.needed - (have.quantity + have.foil_quantity)
    if (short <= 0) continue
    plan.push({
      id: entry.card.id,
      quantity: have.quantity + short,
      foil_quantity: have.foil_quantity,
    })
  }
  return plan
}

function plural(n: number, noun: string, plural = `${noun}s`): string {
  return `${n} ${n === 1 ? noun : plural}`
}
