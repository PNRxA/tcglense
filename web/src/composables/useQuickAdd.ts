import { computed, type Ref } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { getCardNames, listProducts, type ProductPage } from '@/lib/api'

/** Public suggestion queries for the collection/wish-list and deck quick-add boxes.
 * Exact-name printing discovery lives in `usePrintings`, shared with replacement/scanner. */

/** Minimum characters before the quick-add box queries for name hints — short
 * enough to feel responsive, long enough to keep the suggestion set tight. */
export const QUICK_ADD_MIN_CHARS = 2

/** Distinct card-name hints for the quick-add box. `term` is the (already debounced)
 * search text; the query only runs once its trimmed length is at least
 * {@link QUICK_ADD_MIN_CHARS}, so a one-character term never fires a broad lookup. */
export function useCardNameSuggestions(game: Ref<string>, term: Ref<string>) {
  const trimmed = computed(() => term.value.trim())
  const enabled = computed(() => trimmed.value.length >= QUICK_ADD_MIN_CHARS)
  return useQuery({
    queryKey: ['card-names', game, trimmed],
    queryFn: () => getCardNames(game.value, trimmed.value),
    enabled,
    // Keep the last hints visible while the next keystroke's query resolves.
    placeholderData: keepPreviousData,
    // Names change at most daily, so a short cache spares a refetch when the user
    // backspaces to a term they just typed.
    staleTime: 60_000,
  })
}

/** Sealed-product suggestions for the product quick-add box: a small name-matched page of
 * the public products list (order-independent word-AND substring match, name order). */
export function useProductSuggestions(game: Ref<string>, term: Ref<string>) {
  const trimmed = computed(() => term.value.trim())
  const enabled = computed(() => trimmed.value.length >= QUICK_ADD_MIN_CHARS)
  return useQuery<ProductPage>({
    queryKey: ['product-suggest', game, trimmed],
    queryFn: () => listProducts(game.value, { q: trimmed.value, pageSize: 10 }),
    enabled,
    placeholderData: keepPreviousData,
    staleTime: 60_000,
  })
}
