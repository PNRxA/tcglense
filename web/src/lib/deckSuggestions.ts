import type { DeckSuggestionCard, DeckSuggestions } from '@/lib/api'

// The small pure half of the "from your collection" panel (issue #684): how many of the most
// popular owned cards the resting panel names, and how the filters the server applied are
// worded. Kept out of the component so the spec can pin the wording without mounting it —
// and because the wording carries an honesty claim: the panel must say what it filtered by
// and, when the server applied no filter, say that too, never imply a fit it didn't check.

/** How many of the most popular candidates the collapsed panel lists. */
export const TOP_SUGGESTED = 6

/** `#42` — a card's EDHREC rank as the row prints it. */
export function rankLabel(card: Pick<DeckSuggestionCard, 'edhrec_rank'>): string {
  return `#${card.edhrec_rank.toLocaleString()}`
}

/** The colour filter the server applied, worded for the header: whose colours, and which.
 * `null` is the server saying it applied none (the deck had no card to read a colour off);
 * an empty list is a colourless deck, which only colourless cards fit. */
export function colourFilterLabel(
  suggestions: Pick<DeckSuggestions, 'color_identity' | 'commanders'>,
): string {
  const identity = suggestions.color_identity
  if (identity == null) return 'any colour (the deck has no cards to read a colour off)'
  const letters = identity.length ? identity.join('') : 'colourless'
  const owners = suggestions.commanders.map((c) => c.name)
  if (owners.length === 0) return `in the deck's colours (${letters})`
  const whose = owners.length === 1 ? owners[0] : owners.join(' + ')
  return `in ${whose}'s colours (${letters})`
}

/** The legality filter the server applied, worded for the header. */
export function formatFilterLabel(suggestions: Pick<DeckSuggestions, 'format_label'>): string {
  return suggestions.format_label
    ? `legal in ${suggestions.format_label}`
    : 'any format (the deck’s format isn’t tracked)'
}

/** The one-line summary under the panel title: how many fit, and both filters. */
export function suggestionsSummary(suggestions: DeckSuggestions): string {
  const n = suggestions.candidate_count
  const fit = n === 1 ? '1 card you own fits' : `${n.toLocaleString()} cards you own fit`
  return `${fit} · ${colourFilterLabel(suggestions)} · ${formatFilterLabel(suggestions)}`
}
