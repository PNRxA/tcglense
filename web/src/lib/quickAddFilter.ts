import type { Card } from './api'

// Client-side filter for the quick-add print picker. A card can have many printings, so
// the dialog offers a box to narrow them by set name/code (e.g. "TLA"), collector number
// (e.g. "#2672" or "2672"), rarity, or language. Kept here (not inline in the component)
// so the matching rules are unit-tested.

/** The lowercased text a printing is matched against. */
function printHaystack(card: Card): string {
  return [
    card.set_name,
    card.set_code,
    card.collector_number,
    // Include the `#`-prefixed form so a query typed as "#2672" matches as-is.
    `#${card.collector_number}`,
    card.rarity ?? '',
    card.lang,
  ]
    .join(' ')
    .toLowerCase()
}

/**
 * Filter printings by a free-text query: whitespace-separated tokens are ANDed, each a
 * case-insensitive substring of the card's set name/code, collector number, rarity, or
 * language. A blank query returns the list unchanged.
 */
export function filterPrintings(cards: Card[], query: string): Card[] {
  const tokens = query.trim().toLowerCase().split(/\s+/).filter(Boolean)
  if (!tokens.length) return cards
  return cards.filter((card) => {
    const haystack = printHaystack(card)
    return tokens.every((token) => haystack.includes(token))
  })
}
