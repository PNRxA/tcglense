// Card-grid display sizes for the catalog browse pages. The user picks a size and
// it persists across sessions (see stores/cardSize), changing how many cards pack
// into each row of the grid — smaller cards mean more per row.

export type CardSize = 'small' | 'medium' | 'large'

export interface CardSizeOption {
  value: CardSize
  label: string
}

/** Offered in the size menu, smallest (densest) first. */
export const CARD_SIZE_OPTIONS: readonly CardSizeOption[] = [
  { value: 'small', label: 'Small' },
  { value: 'medium', label: 'Medium' },
  { value: 'large', label: 'Large' },
]

/** Medium keeps the original grid density, so it's the default. */
export const DEFAULT_CARD_SIZE: CardSize = 'medium'

export function isCardSize(value: unknown): value is CardSize {
  return value === 'small' || value === 'medium' || value === 'large'
}

// Responsive column counts per size, keyed off the same breakpoints the grid has
// always used (medium reproduces the original layout exactly). These are written
// as complete, literal class strings on purpose: Tailwind's scanner only generates
// utilities it can see verbatim in the source, so a computed/interpolated class
// like `grid-cols-${n}` would silently produce no CSS.
export const CARD_SIZE_GRID_CLASS: Record<CardSize, string> = {
  small: 'grid-cols-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-7 xl:grid-cols-8',
  medium: 'grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6',
  large: 'grid-cols-2 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-4',
}
