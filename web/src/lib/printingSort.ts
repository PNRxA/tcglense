import type { Card } from '@/lib/api'
import { displayUsdPrice } from '@/lib/cardPrice'
import { type SortOption, toSortParam } from '@/lib/cardSort'

// Client-side sort for the visual printing surfaces: the quick-add / deck print pickers (the
// shared `PrintingPickerGrid`) and the card page's "Other printings" section. Every one of
// these lists printings of a *single* card, so only the metadata that varies across printings
// is worth sorting on — release date, set, collector number, rarity and price; name and mana
// value are identical across a card's printings, so those sorts are deliberately omitted. Kept
// out of the components (like `quickAddFilter`) so the ordering rules are shared and unit-tested.

export const PRINTING_SORT_OPTIONS: SortOption[] = [
  { value: 'released:desc', label: 'Newest first' },
  { value: 'released:asc', label: 'Oldest first' },
  { value: 'set:asc', label: 'Set (A–Z)' },
  { value: 'set:desc', label: 'Set (Z–A)' },
  { value: 'number:asc', label: 'Collector number' },
  { value: 'rarity:desc', label: 'Rarity (high → low)' },
  { value: 'rarity:asc', label: 'Rarity (low → high)' },
  { value: 'price:desc', label: 'Price (high → low)' },
  { value: 'price:asc', label: 'Price (low → high)' },
]

/** Default printing order — newest printing first, matching the `/prints` endpoint's order
 * and the picker's server order, so the default is a no-op reordering (a stable sort leaves
 * equal-date printings in the order the API returned them). */
export const PRINTING_DEFAULT_SORT = 'released:desc'

// Rarity low→high ordinal, mirroring the backend's `scryfall::search::RARITIES` so the client
// sort and the API's `sort=rarity` agree. An unknown/absent rarity ranks last in either
// direction (it maps to `null`, which `compareNullable` parks at the end).
const RARITY_RANK: Record<string, number> = {
  common: 0,
  uncommon: 1,
  rare: 2,
  special: 3,
  mythic: 4,
  bonus: 5,
}

type Dir = 'asc' | 'desc'

/** Compare two nullable numbers, honouring `dir`, with `null` always last (in either
 * direction) — the same missing-values-last rule the backend's `NULLS LAST` applies. */
function compareNullable(a: number | null, b: number | null, dir: Dir): number {
  if (a === null && b === null) return 0
  if (a === null) return 1
  if (b === null) return -1
  const cmp = a - b
  return dir === 'desc' ? -cmp : cmp
}

/** Compare two strings (present or `null`), honouring `dir`, with `null`/empty last. */
function compareNullableStr(a: string | null, b: string | null, dir: Dir): number {
  if (!a && !b) return 0
  if (!a) return 1
  if (!b) return -1
  const cmp = a.localeCompare(b)
  return dir === 'desc' ? -cmp : cmp
}

/** The leading integer of a collector number (`"18a"` → 18), or `null` for a non-numeric
 * one (`"★"`) so it sorts last, matching the backend's `collector_number_int` ordering. */
function collectorInt(card: Card): number | null {
  const match = /^\d+/.exec(card.collector_number)
  return match ? Number.parseInt(match[0], 10) : null
}

/** A printing's sort price: its displayed USD (regular, else foil) as a number, or `null`
 * when unpriced — mirroring the tiles' shown price and the backend's price fallback. */
function priceValue(card: Card): number | null {
  const price = displayUsdPrice(card.prices)
  if (!price) return null
  const amount = Number.parseFloat(price.amount)
  return Number.isNaN(amount) ? null : amount
}

function rarityRank(card: Card): number | null {
  return card.rarity ? (RARITY_RANK[card.rarity.toLowerCase()] ?? null) : null
}

function compareBy(field: string, dir: Dir, a: Card, b: Card): number {
  switch (field) {
    case 'set':
      return compareNullableStr(a.set_code, b.set_code, dir)
    case 'number':
      return compareNullable(collectorInt(a), collectorInt(b), dir)
    case 'rarity':
      return compareNullable(rarityRank(a), rarityRank(b), dir)
    case 'price':
      return compareNullable(priceValue(a), priceValue(b), dir)
    case 'released':
    default:
      return compareNullableStr(a.released_at, b.released_at, dir)
  }
}

/**
 * Sort a card's printings by a `field:dir` value from `PRINTING_SORT_OPTIONS` (parsed with
 * the shared `toSortParam`). Returns a new array; `Array.prototype.sort` is stable (ES2019+),
 * so printings tied on the sort key keep their incoming order — the API's order for the
 * picker, the `/prints` order for the card page. A blank/unknown value falls back to the
 * newest-first default.
 */
export function sortPrintings(cards: Card[], value: string): Card[] {
  const { sort, dir } = toSortParam(value, PRINTING_DEFAULT_SORT)
  return [...cards].sort((a, b) => compareBy(sort, dir, a, b))
}
