import { computed, type Ref } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { getCardNames, getCardPrintingsByName, type CardPage } from '@/lib/api'

/**
 * Server state for the collection quick-add box (`GameCollectionView`): the
 * distinct card-name hints the text box suggests, and every printing of a chosen
 * name to pick from. Both are public catalog reads, so they use plain `useQuery`
 * (like `useCatalog`) rather than the authed wrapper — the actual add is what's
 * authenticated, and it goes through the existing collection mutation.
 */

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

/** Every printing of the exact card `name`, newest printing first — the quick-add
 * "pick which printing" step. `opts.enabled` lets the caller defer the fetch until
 * the print picker is actually open (so choosing a name is what triggers it). */
export function useCardPrintingsByName(
  game: Ref<string>,
  name: Ref<string>,
  opts: { enabled?: Ref<boolean> } = {},
) {
  const enabled = computed(() => name.value.length > 0 && (opts.enabled?.value ?? true))
  return useQuery<CardPage>({
    queryKey: ['card-printings', game, name],
    queryFn: () => getCardPrintingsByName(game.value, name.value),
    enabled,
    staleTime: 60_000,
  })
}
