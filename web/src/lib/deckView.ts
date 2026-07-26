// How a deck's card list is rendered (issue #570). Deck pages have always shown one
// thing: a grid of card images. That's the right default for building and browsing, but
// it's a poor fit for the two other jobs a deck list does — scanning a 100-card list for
// what's in it, and reading/copying it as text. So the section body is a choice:
//
//   grid — the original image tiles (see DECK_CARD_SIZE_GRID_CLASS)
//   list — one compact row per card: name, mana cost, type, price, controls
//   text — just "4 Sol Ring", the decklist as plain text
//
// The choice is a personal display preference like the card size, so it persists in
// localStorage (see stores/deckView) rather than the URL, and applies to both the owner
// and public deck views.

export type DeckViewMode = 'grid' | 'list' | 'text'

export interface DeckViewModeOption {
  value: DeckViewMode
  label: string
}

/** Offered in the view menu, richest first. */
export const DECK_VIEW_MODE_OPTIONS: readonly DeckViewModeOption[] = [
  { value: 'grid', label: 'Card images' },
  { value: 'list', label: 'Compact list' },
  { value: 'text', label: 'Text list' },
]

/** The image grid is the historical deck layout, so it stays the default. */
export const DEFAULT_DECK_VIEW_MODE: DeckViewMode = 'grid'

export function isDeckViewMode(value: unknown): value is DeckViewMode {
  return value === 'grid' || value === 'list' || value === 'text'
}
