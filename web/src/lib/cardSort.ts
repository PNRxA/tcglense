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

/** Options for the all-cards view — the same card sorts a single set offers, collector
 * number included. Across sets it groups each set's run together (numeric run first, then
 * the raw string); `name` stays the *default* here, but the option is always offered so a
 * collector-number sort never vanishes when the collection / wish-list ghost toggle swaps
 * this list in for `COLLECTION_SORT_OPTIONS` (or vice-versa). */
export const ALL_CARDS_SORT_OPTIONS: SortOption[] = [...SET_SORT_OPTIONS]

/** Options for the collection view — also reused by the wish list (its "want" twin) for
 * its list-only mode. The holdings-specific sorts lead: the recency order (the natural
 * default — how it has always sorted) and a total-copies-held sort (issue #228), then the
 * same card sorts the all-cards view offers. `updated:*` and `quantity:*` map to the
 * backend's holdings-only `updated` / `quantity` sort keys (the latter ordering by regular
 * + foil copies). */
export const COLLECTION_SORT_OPTIONS: SortOption[] = [
  { value: 'updated:desc', label: 'Recently updated' },
  { value: 'updated:asc', label: 'Least recently updated' },
  { value: 'quantity:desc', label: 'Quantity (high → low)' },
  { value: 'quantity:asc', label: 'Quantity (low → high)' },
  ...ALL_CARDS_SORT_OPTIONS,
]

/** Options for the sealed-products browse view. The API sorts on `name`/`price`/
 * `released` only (no rarity/mana/collector-number — sealed products have none). */
export const PRODUCT_SORT_OPTIONS: SortOption[] = [
  { value: 'name:asc', label: 'Name (A–Z)' },
  { value: 'name:desc', label: 'Name (Z–A)' },
  { value: 'price:desc', label: 'Price (high → low)' },
  { value: 'price:asc', label: 'Price (low → high)' },
  { value: 'released:desc', label: 'Newest first' },
  { value: 'released:asc', label: 'Oldest first' },
]

/** Default sort for the sealed-products view — alphabetical by name. */
export const PRODUCT_DEFAULT_SORT = 'name:asc'

/** Set code whose sealed products default to newest-first instead of name-A→Z. Secret Lair
 * (`sld`) is a rolling series of individually-dated drops — all filed under one TCGCSV group —
 * so "newest first" is the useful default there; every other set releases at once, where
 * alphabetical reads better. */
const NEWEST_FIRST_SET_CODE = 'sld'

/** The default product sort for a (possibly set-scoped) sealed browse: newest-first for Secret
 * Lair (`sld`), else the name-A→Z default. `setCode` is the active set filter — empty for the
 * all-products view, which keeps the name default. Pure so a view can wrap it in a `computed`
 * keyed on the active set and feed it to `useCardSearch`'s reactive `defaultSort`. */
export function productDefaultSortFor(setCode: string): string {
  return setCode === NEWEST_FIRST_SET_CODE ? 'released:desc' : PRODUCT_DEFAULT_SORT
}

/** Sort options for the cards **inside** a sealed product (distinct from the sealed-products
 * browse's `PRODUCT_SORT_OPTIONS` above, which sorts the products themselves). A product's
 * cards are a curated group like a set's, so this mirrors a set's card sorts — but the default
 * keeps the product's own membership / exclusive / set-number order: the `default` sentinel
 * maps to the API's natural order (no `sort` param sent), so the section split and the
 * exclusives-first ordering stay intact. Every other option re-orders the cards *within* each
 * section. */
export const PRODUCT_CARDS_SORT_OPTIONS: SortOption[] = [
  { value: 'default', label: 'Default order' },
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

/** Default sort for a sealed product's cards — the product's natural membership / set-number
 * order. A sentinel that maps to *no* `sort` param, so it is never sent to the API (and never
 * passed to `toSortParam`); an unknown `?sort=` in the URL clamps back to it. */
export const PRODUCT_CARDS_DEFAULT_SORT = 'default'

/** Default sort for a single set — its collector-number order. */
export const SET_DEFAULT_SORT = 'number:asc'
/** Default sort for the all-cards view — alphabetical by name. */
export const ALL_CARDS_DEFAULT_SORT = 'name:asc'
/** Default sort for the collection — most-recently-updated first. */
export const COLLECTION_DEFAULT_SORT = 'updated:desc'

/** Split a `field:dir` option value into the API's `sort`/`dir` params, falling
 * back to `fallback` for an empty value and defaulting an absent/odd direction
 * to ascending. */
export function toSortParam(value: string, fallback: string): SortParam {
  const parts = (value || fallback).split(':')
  return { sort: parts[0] || 'name', dir: parts[1] === 'desc' ? 'desc' : 'asc' }
}
