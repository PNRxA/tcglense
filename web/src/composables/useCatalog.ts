import { computed, type ComputedRef, type Ref } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { listGames, listSets } from '@/lib/api'

/**
 * Shared catalog reads. The game registry and per-game set list are used across the
 * catalog nav + views, so they're centralised here to share one query key (and thus
 * one warm cache entry) rather than being re-declared inline in each consumer.
 */

/** The supported-games registry. Effectively static, so it never goes stale. */
export function useGamesQuery() {
  return useQuery({
    queryKey: ['games'],
    queryFn: () => listGames(),
    staleTime: Infinity,
  })
}

/** A game's display name from the (cached) registry, falling back to its
 * upper-cased id until the registry loads or for an unknown game. */
export function useGameName(game: Ref<string>): ComputedRef<string> {
  const gamesQuery = useGamesQuery()
  return computed(
    () =>
      gamesQuery.data.value?.data.find((g) => g.id === game.value)?.name ??
      game.value.toUpperCase(),
  )
}

/** A game's full set list. Keyed on `['sets', game]` so GameView, SetView and the
 * grouping composable all read the same warm entry. */
export function useSetsQuery(game: Ref<string>) {
  return useQuery({
    queryKey: ['sets', game],
    queryFn: () => listSets(game.value),
  })
}
