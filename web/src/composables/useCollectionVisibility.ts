import type { Ref } from 'vue'
import { useQueryClient } from '@tanstack/vue-query'
import { getCollectionVisibility, setCollectionVisibility } from '@/lib/api'
import type { ApiError, CollectionVisibility } from '@/lib/api'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'

// Server state for a game's public-sharing toggle (issues #361/#362). Owner-only and
// authed; the mutation invalidates the visibility query so the card re-renders.

/** Variables for a visibility write: which game, and the desired public state. */
export interface SetVisibilityVars {
  game: string
  public: boolean
}

/** Whether the signed-in user's collection for a game is public, plus their handle. */
export function useCollectionVisibilityQuery(game: Ref<string>) {
  const options = {
    // Reactive `game` ref inside the key so switching games refetches.
    queryKey: ['collection-visibility', game],
    queryFn: (token: string) => getCollectionVisibility(token, game.value),
    staleTime: 60_000,
  }
  return useAuthedQuery<CollectionVisibility>(options)
}

/** Enable/disable public sharing for a game; invalidates the visibility query. */
export function useSetCollectionVisibilityMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: SetVisibilityVars) =>
      setCollectionVisibility(token, vars.game, vars.public),
    onSettled: (_data: CollectionVisibility | undefined, _error: ApiError | null, vars: SetVisibilityVars) =>
      qc.invalidateQueries({ queryKey: ['collection-visibility', vars.game] }),
  }
  return useAuthedMutation<CollectionVisibility, SetVisibilityVars>(options)
}
