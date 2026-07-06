import type { Card } from './api'

// Client-side filter for the quick-add print picker. A card can have many printings, so
// the dialog offers a box to narrow them by set name/code (e.g. "TLA"), collector number
// (e.g. "#2672" or "2672"), rarity, or language. Kept here (not inline in the component)
// so the matching rules are unit-tested.

/** Drop leading zeros from a run of digits, keeping a lone "0" (so "0001" → "1", "0" → "0"). */
function stripLeadingZeros(digits: string): string {
  return digits.replace(/^0+(?=\d)/, '')
}

/** A purely-numeric collector number normalized for exact comparison, or null otherwise. */
function normalizedCollectorNumber(collectorNumber: string): string | null {
  return /^\d+$/.test(collectorNumber) ? stripLeadingZeros(collectorNumber) : null
}

/** The lowercased free-text a printing is matched against for non-numeric tokens. */
function printHaystack(card: Card): string {
  return [card.set_name, card.set_code, card.collector_number, card.rarity ?? '', card.lang]
    .join(' ')
    .toLowerCase()
}

/**
 * Match one already-lowercased token against a printing.
 *
 * A bare number — optionally `#`-prefixed, e.g. "2672", "#2672", "0001" — is a collector
 * number lookup: it matches the collector number *exactly*, leading zeros ignored on both
 * sides, so "1" finds the printing numbered 1, not 10, 18 or 100 (#268). Failing that it
 * still matches a *standalone* number in the set name/code (word boundaries, never a
 * substring) so "2022" keeps finding "Double Masters 2022" while "1" stays clear of "2021".
 *
 * Any other token (a `#`-prefix aside — collector numbers can carry a letter suffix or a
 * "★", so "#18a" must still work) is a plain case-insensitive substring of the haystack.
 */
function matchesToken(
  card: Card,
  haystack: string,
  cardNumber: string | null,
  token: string,
): boolean {
  // A leading '#' only ever prefixes a collector number; the haystack has none, so drop it.
  const bare = token.startsWith('#') ? token.slice(1) : token
  if (!/^\d+$/.test(bare)) return haystack.includes(bare)
  if (cardNumber === stripLeadingZeros(bare)) return true
  return new RegExp(`\\b${bare}\\b`).test(`${card.set_name} ${card.set_code}`.toLowerCase())
}

/**
 * Filter printings by a free-text query: whitespace-separated tokens are ANDed, each
 * matched per `matchesToken`. A blank query returns the list unchanged.
 */
export function filterPrintings(cards: Card[], query: string): Card[] {
  const tokens = query.trim().toLowerCase().split(/\s+/).filter(Boolean)
  if (!tokens.length) return cards
  return cards.filter((card) => {
    const haystack = printHaystack(card)
    const cardNumber = normalizedCollectorNumber(card.collector_number)
    return tokens.every((token) => matchesToken(card, haystack, cardNumber, token))
  })
}
