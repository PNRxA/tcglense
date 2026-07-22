import type { DeckCardEntry } from './api'
import {
  matchesPrintingToken,
  printingTokenContext,
  type PrintingTokenContext,
} from './quickAddFilter'

// Client-side filter for the deck views (issue #562). A deck's full card list is already
// on the page (the detail payload carries every entry), so filtering is a pure re-shape of
// loaded data — no server round-trip. Kept here (not inline in the views) so the matching
// rules are unit-tested. Text tokens match the card's gameplay text OR its printing
// metadata — the latter via `quickAddFilter`'s shared token rules, so set/number/rarity
// queries behave exactly like the printing picker's filter box.

/** One selectable colour pip: the five colours plus colourless. */
export type DeckFilterColor = 'W' | 'U' | 'B' | 'R' | 'G' | 'C'

export interface DeckFilterColorOption {
  value: DeckFilterColor
  label: string
  /** The mana-font glyph class rendered on the toggle pip. */
  icon: string
}

/** Offered in the colour filter row, WUBRG order with colourless last. */
export const DECK_FILTER_COLOR_OPTIONS: readonly DeckFilterColorOption[] = [
  { value: 'W', label: 'White', icon: 'ms-w' },
  { value: 'U', label: 'Blue', icon: 'ms-u' },
  { value: 'B', label: 'Black', icon: 'ms-b' },
  { value: 'R', label: 'Red', icon: 'ms-r' },
  { value: 'G', label: 'Green', icon: 'ms-g' },
  { value: 'C', label: 'Colorless', icon: 'ms-c' },
]

/** The gameplay half of the text haystack: name, type line, and rules text, including
 * every face of a multi-faced card (so an MDFC's back face is findable too). Printing
 * metadata (set/number/rarity/language) is matched via `quickAddFilter`'s context. */
function entryHaystack(entry: DeckCardEntry): string {
  const card = entry.card
  const parts = [card.name, card.type_line ?? '', card.oracle_text ?? '']
  for (const face of card.faces) {
    parts.push(face.name ?? '', face.type_line ?? '', face.oracle_text ?? '')
  }
  return parts.join('\n').toLowerCase()
}

/**
 * Whether an entry's colour identity meets the pip selection. An empty selection is no
 * constraint; otherwise the selected pips are ORed — a card matches if its identity shares
 * any selected colour, or if it's colourless and the colourless pip is selected. Colour
 * *identity* (not just cast cost) so lands and off-cost cards group with their colours,
 * matching the analytics panel's "Colour identity" bars.
 */
function matchesColors(entry: DeckCardEntry, colors: readonly DeckFilterColor[]): boolean {
  if (colors.length === 0) return true
  const identity = entry.card.color_identity
  if (identity.length === 0) return colors.includes('C')
  return identity.some((color) => (colors as readonly string[]).includes(color))
}

/**
 * Match one already-lowercased token against an entry's gameplay text or its printing
 * metadata. A bare number keeps the printing picker's exact semantics (the collector
 * number with leading zeros ignored, or a standalone number in the set name/code) and
 * additionally matches a *standalone* number in the gameplay text ("deals 3 damage") —
 * never a substring, so "1" stays clear of "100". A `#`-prefixed number is the picker's
 * established collector-number syntax, so it stays a pure printing lookup and never falls
 * through to rules text. Any other token is a plain substring of either haystack (set
 * name/code, rarity, and language ride the printing one).
 */
function matchesTextToken(
  entry: DeckCardEntry,
  gameplay: string,
  printing: PrintingTokenContext,
  token: string,
): boolean {
  const bare = token.startsWith('#') ? token.slice(1) : token
  if (/^\d+$/.test(bare)) {
    return (
      matchesPrintingToken(entry.card, printing, token) ||
      (!token.startsWith('#') && new RegExp(`\\b${bare}\\b`).test(gameplay))
    )
  }
  return gameplay.includes(token) || matchesPrintingToken(entry.card, printing, token)
}

/**
 * Filter deck entries by a free-text query and a colour-pip selection, ANDed together.
 * Whitespace-separated query tokens are ANDed, each matched per `matchesTextToken`.
 * Blank query + no pips returns the list unchanged.
 */
export function filterDeckEntries(
  entries: DeckCardEntry[],
  query: string,
  colors: readonly DeckFilterColor[],
): DeckCardEntry[] {
  const tokens = query.trim().toLowerCase().split(/\s+/).filter(Boolean)
  if (tokens.length === 0 && colors.length === 0) return entries
  return entries.filter((entry) => {
    if (!matchesColors(entry, colors)) return false
    if (tokens.length === 0) return true
    const gameplay = entryHaystack(entry)
    const printing = printingTokenContext(entry.card)
    return tokens.every((token) => matchesTextToken(entry, gameplay, printing, token))
  })
}
