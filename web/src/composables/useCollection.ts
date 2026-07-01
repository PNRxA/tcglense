import { computed, type Ref } from 'vue'
import { keepPreviousData, useQueryClient } from '@tanstack/vue-query'
import {
  getCollection,
  getCollectionEntry,
  getCollectionSummary,
  setCollectionEntry,
  type ApiError,
  type CollectionPage,
  type CollectionQuantities,
  type CollectionSummary,
} from '@/lib/api'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'
import { useAuthStore } from '@/stores/auth'

/**
 * Server state for the signed-in user's card collection. Reads go through
 * `useAuthed*` (which routes via the auth store's `authFetch`, refreshing an expired
 * access token transparently); writes invalidate the dependent reads so the list,
 * summary, and per-card controls stay in sync after an edit.
 *
 * The option objects are built as intermediate variables rather than inline literals
 * on purpose: TanStack's deeply-reactive option types make an inline literal trip
 * excess-property checks through the `useAuthed*` wrappers (see `lib/queries.ts`), so
 * a plain variable (with explicit callback param types) is the clean way to pass them.
 */

/** Cards per page in the collection grid (matches the catalog default). */
export const COLLECTION_PAGE_SIZE = 60

/** A page of the user's owned cards for a game. `page` is reactive (paginated view).
 * Disabled while signed out — the collection routes are public, so a signed-out
 * visitor lands here (and is prompted to sign in) without triggering an auth call. */
export function useCollectionQuery(game: Ref<string>, page: Ref<number>) {
  const auth = useAuthStore()
  const options = {
    queryKey: ['collection', game, page],
    queryFn: (token: string) =>
      getCollection(token, game.value, { page: page.value, pageSize: COLLECTION_PAGE_SIZE }),
    // Keep the current grid visible while the next page loads (smoother paging).
    placeholderData: keepPreviousData,
    enabled: computed(() => auth.isAuthenticated),
  }
  return useAuthedQuery<CollectionPage>(options)
}

/** Aggregate stats (unique cards, total copies, estimated value) for the collection.
 * Disabled while signed out (see `useCollectionQuery`). */
export function useCollectionSummaryQuery(game: Ref<string>) {
  const auth = useAuthStore()
  const options = {
    queryKey: ['collection-summary', game],
    queryFn: (token: string) => getCollectionSummary(token, game.value),
    enabled: computed(() => auth.isAuthenticated),
  }
  return useAuthedQuery<CollectionSummary>(options)
}

/**
 * How many copies of one card the signed-in user owns — for the card-detail
 * controls. Disabled while signed out (the route is public), so a logged-out
 * visitor never triggers an auth call.
 */
export function useCollectionEntryQuery(game: Ref<string>, id: Ref<string>) {
  const auth = useAuthStore()
  const options = {
    queryKey: ['collection-entry', game, id],
    queryFn: (token: string) => getCollectionEntry(token, game.value, id.value),
    enabled: computed(() => auth.isAuthenticated),
  }
  return useAuthedQuery<CollectionQuantities>(options)
}

/** Variables for a collection write: which card, and the desired absolute counts. */
export interface SetCollectionVars {
  game: string
  id: string
  quantity: number
  foil_quantity: number
}

/**
 * Set the owned counts for a card. On success the per-card cache is updated
 * immediately; on settle the list + summary (+ that card's entry) are invalidated
 * so every dependent view refreshes.
 */
export function useSetCollectionEntryMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: SetCollectionVars) =>
      setCollectionEntry(token, vars.game, vars.id, {
        quantity: vars.quantity,
        foil_quantity: vars.foil_quantity,
      }),
    onSuccess: (data: CollectionQuantities, vars: SetCollectionVars) => {
      qc.setQueryData(['collection-entry', vars.game, vars.id], data)
    },
    onSettled: (
      _data: CollectionQuantities | undefined,
      _error: ApiError | null,
      vars: SetCollectionVars,
    ) => {
      qc.invalidateQueries({ queryKey: ['collection', vars.game] })
      qc.invalidateQueries({ queryKey: ['collection-summary', vars.game] })
      qc.invalidateQueries({ queryKey: ['collection-entry', vars.game, vars.id] })
    },
  }
  return useAuthedMutation<CollectionQuantities, SetCollectionVars>(options)
}
