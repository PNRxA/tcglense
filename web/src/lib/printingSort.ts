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

/**
 * Held-first ordering: the printings the signed-in user already has on the target list float
 * above the ones they don't, each group keeping the default newest-first order. It is a *sort
 * option* rather than an unconditional reordering so an explicit pick from the sort menu
 * (price, set, collector number…) still means exactly what it says.
 *
 * It takes a **set of ids**, not the live counts map, and that is the load-bearing part: the
 * counts a picker shows are a value the user edits in place, so ordering off them would float
 * the tile under the pointer to the top mid-click, and would resnap the whole grid every time
 * the counts query refetched. The caller decides held-ness once per printing and hands over a
 * set that only ever grows (see `QuickAddPrintDialog`), so the order is settled by the time
 * anything is clickable and never moves again.
 */
export const PRINTING_HELD_FIRST_SORT = 'held:desc'

/** `PRINTING_SORT_OPTIONS` led by the held-first option. `label` names the target list in the
 * caller's words ("Owned first" for the collection, its wish-list wording on the twin), since
 * the set handed to `sortPrintings` is whichever list that caller is adding to. */
export function heldFirstSortOptions(label: string): SortOption[] {
  return [{ value: PRINTING_HELD_FIRST_SORT, label }, ...PRINTING_SORT_OPTIONS]
}

/** 1 when this printing is one of the held ones, else 0. */
function heldRank(card: Card, held: ReadonlySet<string> | undefined): number {
  return held?.has(card.id) ? 1 : 0
}

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

function compareBy(
  field: string,
  dir: Dir,
  a: Card,
  b: Card,
  held: ReadonlySet<string> | undefined,
): number {
  switch (field) {
    case 'held': {
      // Held-first is a *grouping*, not a full order: within each group the printings keep
      // the newest-first default, so the held block and the rest each read the way the
      // unsorted list did. With no set (or an empty one) every printing ranks equal and this
      // collapses to exactly that default.
      const cmp = compareNullable(heldRank(a, held), heldRank(b, held), dir)
      return cmp !== 0 ? cmp : compareNullableStr(a.released_at, b.released_at, 'desc')
    }
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
 *
 * `held` is only read by `PRINTING_HELD_FIRST_SORT`; omitting it (every caller that can't know
 * what the user holds) leaves that value ordering by the newest-first default.
 */
export function sortPrintings(cards: Card[], value: string, held?: ReadonlySet<string>): Card[] {
  const { sort, dir } = toSortParam(value, PRINTING_DEFAULT_SORT)
  return [...cards].sort((a, b) => compareBy(sort, dir, a, b, held))
}
