import { computed, type Ref } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { getCardNames, listProducts, type Card, type ProductPage } from '@/lib/api'
import type { OwnedCountWrite } from '@/composables/useOwnedCountEditor'

/** Public suggestion queries for the collection/wish-list and deck quick-add boxes.
 * Exact-name printing discovery lives in `usePrintings`, shared with replacement/scanner. */

/** What a quick-add print tile reports once one of its saves lands: the write itself plus
 * the printing it was for. It travels tile → dialog → box, where the box turns it into an
 * optional `saved` event every other host simply ignores. The scan page listens: a manual add
 * is written to the same collection the scanner writes to, so it belongs in the same session
 * history, with the same one-tap undo. */
export interface QuickAddSaved extends OwnedCountWrite {
  card: Card
}

/**
 * How that report travels down: a plain **callback prop**, not an event, on both inner hops.
 *
 * A debounced save lands a round-trip after it was scheduled, and the editor deliberately
 * flushes a pending edit on unmount — so the reporting call routinely happens *after* the
 * tile (and the dialog around it) have gone: tap `+`, tap Done. Vue's `emit()` returns early
 * once an instance is unmounted, so an event there is silently dropped and the write lands on
 * the server with no history row, no undo, and no rebase of an open tentative match. A
 * captured function has no such guard. Only the last hop — the box, which outlives the
 * dialog it opens — reports through a normal event.
 */
export type QuickAddSavedReporter = (write: QuickAddSaved) => void

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
