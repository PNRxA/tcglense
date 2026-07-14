import type { Ref } from 'vue'
import { useQueryClient } from '@tanstack/vue-query'
import { getCollectionVisibility, setCollectionVisibility } from '@/lib/api'
import type { ApiError, CollectionVisibility, CollectionVisibilityPatch } from '@/lib/api'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'

// Server state for a game's collection visibility (issues #361/#362) and its collection-
// landing display prefs (issue #381). Both live on the same per-(user, game) row and ride
// the same GET/PUT, so they share one query. Owner-only and authed. Writes are partial
// patches applied optimistically — the sharing switch and the section toggles feel
// instant — then reconciled by an invalidate (and rolled back if the write fails).

const keyFor = (game: string) => ['collection-visibility', game]

/** Variables for a visibility/display write: which game, and the partial patch to apply. */
export interface SetVisibilityVars {
  game: string
  patch: CollectionVisibilityPatch
}

/** The signed-in user's visibility + landing display state for a game, plus their handle. */
export function useCollectionVisibilityQuery(game: Ref<string>) {
  const options = {
    // Reactive `game` ref inside the key so switching games refetches.
    queryKey: ['collection-visibility', game],
    queryFn: (token: string) => getCollectionVisibility(token, game.value),
    staleTime: 60_000,
  }
  return useAuthedQuery<CollectionVisibility>(options)
}

/** Patch a game's visibility / display prefs; optimistic, with rollback on error and a
 * settle-time invalidate. */
export function useSetCollectionVisibilityMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: SetVisibilityVars) =>
      setCollectionVisibility(token, vars.game, vars.patch),
    // Optimistically apply the patch so the switch and any gated section move instantly;
    // snapshot the prior value to roll back if the write fails.
    onMutate: async (vars: SetVisibilityVars) => {
      const key = keyFor(vars.game)
      await qc.cancelQueries({ queryKey: key })
      const previous = qc.getQueryData<CollectionVisibility>(key)
      if (previous) qc.setQueryData<CollectionVisibility>(key, { ...previous, ...vars.patch })
      return { previous }
    },
    onError: (_error: ApiError, vars: SetVisibilityVars, context: unknown) => {
      const previous = (context as { previous?: CollectionVisibility } | undefined)?.previous
      if (!previous) return
      // Roll back only the field(s) THIS patch changed, layered onto the current cache — so
      // a sibling patch still in flight (another toggle, or the sharing control's own
      // mutation on the same row) keeps its optimistic value instead of being reverted too.
      qc.setQueryData<CollectionVisibility>(keyFor(vars.game), (current) => {
        const restored = { ...(current ?? previous) }
        for (const key of Object.keys(vars.patch) as (keyof CollectionVisibilityPatch)[]) {
          restored[key] = previous[key]
        }
        return restored
      })
    },
    onSettled: (
      _data: CollectionVisibility | undefined,
      _error: ApiError | null,
      vars: SetVisibilityVars,
    ) => qc.invalidateQueries({ queryKey: keyFor(vars.game) }),
  }
  return useAuthedMutation<CollectionVisibility, SetVisibilityVars>(options)
}
