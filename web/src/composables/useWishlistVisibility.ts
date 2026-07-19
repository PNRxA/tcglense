import type { Ref } from 'vue'
import { useQueryClient } from '@tanstack/vue-query'
import { getWishlistVisibility, setWishlistVisibility } from '@/lib/api'
import type { ApiError, WishlistVisibility, WishlistVisibilityPatch } from '@/lib/api'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'

// Server state for a game's wish-list visibility (issue #493) — the wish-list twin of
// `useCollectionVisibility`, over the independent `wishlist_is_public` flag. A wish list has
// no landing display prefs, so the only knob is public/private. Owner-only and authed. Writes
// are optimistic (the switch feels instant), then reconciled by an invalidate (and rolled back
// if the write fails).

const keyFor = (game: string) => ['wishlist-visibility', game]

/** Variables for a visibility write: which game, and the partial patch to apply. */
export interface SetWishlistVisibilityVars {
  game: string
  patch: WishlistVisibilityPatch
}

/** The signed-in user's wish-list visibility state for a game, plus their handle. */
export function useWishlistVisibilityQuery(game: Ref<string>) {
  const options = {
    // Reactive `game` ref inside the key so switching games refetches.
    queryKey: ['wishlist-visibility', game],
    queryFn: (token: string) => getWishlistVisibility(token, game.value),
    staleTime: 60_000,
  }
  return useAuthedQuery<WishlistVisibility>(options)
}

/** Patch a game's wish-list visibility; optimistic, with rollback on error and a settle-time
 * invalidate. */
export function useSetWishlistVisibilityMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: SetWishlistVisibilityVars) =>
      setWishlistVisibility(token, vars.game, vars.patch),
    onMutate: async (vars: SetWishlistVisibilityVars) => {
      const key = keyFor(vars.game)
      await qc.cancelQueries({ queryKey: key })
      const previous = qc.getQueryData<WishlistVisibility>(key)
      if (previous) qc.setQueryData<WishlistVisibility>(key, { ...previous, ...vars.patch })
      return { previous }
    },
    onError: (_error: ApiError, vars: SetWishlistVisibilityVars, context: unknown) => {
      const previous = (context as { previous?: WishlistVisibility } | undefined)?.previous
      if (!previous) return
      qc.setQueryData<WishlistVisibility>(keyFor(vars.game), (current) => {
        const restored = { ...(current ?? previous) }
        for (const key of Object.keys(vars.patch) as (keyof WishlistVisibilityPatch)[]) {
          restored[key] = previous[key]
        }
        return restored
      })
    },
    onSettled: (
      _data: WishlistVisibility | undefined,
      _error: ApiError | null,
      vars: SetWishlistVisibilityVars,
    ) => qc.invalidateQueries({ queryKey: keyFor(vars.game) }),
  }
  return useAuthedMutation<WishlistVisibility, SetWishlistVisibilityVars>(options)
}
