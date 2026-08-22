// Sort options for the deck list. Unlike the catalog's `cardSort` vocabulary these are
// applied CLIENT-side: the deck list is unpaginated (the API returns every deck
// `updated_at DESC` in one response), so re-ordering it is a pure array sort — no server
// param, no refetch, and the query key stays untouched.

import type { Deck } from '@/lib/api'
import type { SortOption } from '@/lib/cardSort'

export const DECK_SORT_OPTIONS: SortOption[] = [
  { value: 'updated', label: 'Recently updated' },
  { value: 'name', label: 'Name (A–Z)' },
  { value: 'price', label: 'Price (high → low)' },
]

/** Default deck sort — the API's own recency order, passed through untouched. */
export const DECK_DEFAULT_SORT = 'updated'

/** A deck's sortable value in cents, or `null` when nothing in it is priced. `value_usd`
 * is the API's 2-dp decimal string; "unpriced" must stay distinct from `$0.00` so the
 * price sort can park unpriced decks last (the precon browse's server-side rule). */
function valueCents(deck: Deck): number | null {
  if (deck.value_usd == null) return null
  const value = Number.parseFloat(deck.value_usd)
  return Number.isFinite(value) ? Math.round(value * 100) : null
}

/**
 * Order a deck list by the chosen sort, returning a new array (the input — the stable
 * painted order — is never mutated).
 *
 * `updated` returns the input order as-is: the API already sorts by recency and the view
 * pins that order for the mount (`useStableOrder`), so the default deliberately re-imposes
 * nothing. The other sorts are deterministic with full tie-breaks (name, then id), so a
 * background refetch can't reshuffle equal-keyed tiles.
 */
export function sortDecks(decks: readonly Deck[], sort: string): Deck[] {
  const byName = (a: Deck, b: Deck) => a.name.localeCompare(b.name) || a.id - b.id
  switch (sort) {
    case 'name':
      return [...decks].sort(byName)
    case 'price':
      return [...decks].sort((a, b) => {
        const av = valueCents(a)
        const bv = valueCents(b)
        // Most valuable first; unpriced decks sink below every valued one.
        if (av !== bv) {
          if (av === null) return 1
          if (bv === null) return -1
          return bv - av
        }
        return byName(a, b)
      })
    default:
      return [...decks]
  }
}
