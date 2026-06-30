// Sort options for the catalog card-list pages. Each option's `value` is a
// `field:dir` pair matching the API's `sort`/`dir` query params (parsed by
// `toSortParam`), so the dropdown can offer meaningful directions per field
// (e.g. price high→low) while the backend stays a simple orthogonal sort+dir.

export interface SortOption {
  value: string
  label: string
}

export interface SortParam {
  sort: string
  dir: 'asc' | 'desc'
}

/** Options for a single set's cards, led by collector number (the set's order). */
export const SET_SORT_OPTIONS: SortOption[] = [
  { value: 'number:asc', label: 'Collector number' },
  { value: 'name:asc', label: 'Name (A–Z)' },
  { value: 'name:desc', label: 'Name (Z–A)' },
  { value: 'rarity:desc', label: 'Rarity (high → low)' },
  { value: 'rarity:asc', label: 'Rarity (low → high)' },
  { value: 'released:desc', label: 'Newest first' },
  { value: 'released:asc', label: 'Oldest first' },
  { value: 'cmc:asc', label: 'Mana value (low → high)' },
  { value: 'cmc:desc', label: 'Mana value (high → low)' },
  { value: 'price:desc', label: 'Price (high → low)' },
  { value: 'price:asc', label: 'Price (low → high)' },
]

/** Options for the all-cards view: same as a set's, minus collector number
 * (which isn't meaningful across sets). */
export const ALL_CARDS_SORT_OPTIONS: SortOption[] = SET_SORT_OPTIONS.filter(
  (o) => !o.value.startsWith('number:'),
)

/** Default sort for a single set — its collector-number order. */
export const SET_DEFAULT_SORT = 'number:asc'
/** Default sort for the all-cards view — alphabetical by name. */
export const ALL_CARDS_DEFAULT_SORT = 'name:asc'

/** Split a `field:dir` option value into the API's `sort`/`dir` params, falling
 * back to `fallback` for an empty value and defaulting an absent/odd direction
 * to ascending. */
export function toSortParam(value: string, fallback: string): SortParam {
  const parts = (value || fallback).split(':')
  return { sort: parts[0] || 'name', dir: parts[1] === 'desc' ? 'desc' : 'asc' }
}
