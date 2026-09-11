import { computed, type Ref } from 'vue'
import { useOwnedCounts as useCollectionOwnedCounts } from '@/composables/useCollection'
import { useWishlistCounts } from '@/composables/useWishlist'
import type { Card, DeckCardEntry } from '@/lib/api'

// The viewer's collection + wish-list overlay for a deck-shaped card list: how many copies of
// each card they own and want, batched over the list's catalog card ids. It is what draws the
// "you own N / want N" chips (`DeckOwnershipBadges`) beside a deck's own counts, on the
// owner's deck page and on a preconstructed deck's (issue #707) — a precon is catalog data,
// but the reader holding a collection wants to know which of its cards they already have.
//
// Both reads ride the holdings twins' `useCounts` seam, which is gated on being signed in:
// signed out, the maps are empty and every count is zero, so a public page renders no chip
// without a gate of its own. Per-card lookups fold the two finishes, since the chips answer
// "how many copies", not "which finish"; the owner's editor does the split itself when asked.
export function useDeckOwnership(game: Ref<string>, entries: Ref<DeckCardEntry[]>) {
  const catalogCards = computed<Card[]>(() => entries.value.map((entry) => entry.card))
  const { ownership } = useCollectionOwnedCounts(game, catalogCards)
  const { ownership: wishlistWanted } = useWishlistCounts(game, catalogCards)

  /** Copies of the card in the viewer's collection, every finish; 0 while signed out. */
  function ownedInCollection(cardId: string): number {
    const counts = ownership.value[cardId]
    return counts ? counts.quantity + counts.foil_quantity : 0
  }

  /** Copies of the card on the viewer's wish list, every finish; 0 while signed out. */
  function wantedInWishlist(cardId: string): number {
    const counts = wishlistWanted.value[cardId]
    return counts ? counts.quantity + counts.foil_quantity : 0
  }

  return { ownedInCollection, wantedInWishlist }
}
