import type { Ref } from 'vue'
import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/vue-query'
import { copyPrecon, getPrecon, getPreconFacets, listPrecons } from '@/lib/api'
import type { ApiError, DeckDetail, PreconDeckDetail, PreconFacets, PreconPage } from '@/lib/api'
import { useAuthedMutation } from '@/lib/queries'
import { invalidateDeck } from '@/composables/useDecks'
import { PRICED_CATALOG_STALE_MS } from '@/lib/queryClient'

/**
 * Reads for the preconstructed-deck browser. These are PUBLIC catalog endpoints (a precon
 * is game data, like a card or a sealed product), so they use plain `useQuery` rather than
 * `useAuthedQuery`, with the reactive params inside the key so a change refetches.
 *
 * The one write — copying a precon into your own decks — is authed and invalidates the deck
 * family, exactly as the public-deck copy does.
 */

/** Precons per page in the browse grid. */
export const PRECON_PAGE_SIZE = 24

/** Reactive controls for the precon list; all carried in the key. */
interface PreconListQueryOptions {
  page: Ref<number>
  query: Ref<string>
  set: Ref<string>
  type: Ref<string>
  sort: Ref<string>
}

/** A page of a game's precon decks. `keepPreviousData` holds the current grid up while the
 *  next page loads, matching the sealed + card grids. */
export function usePreconsQuery(game: Ref<string>, opts: PreconListQueryOptions) {
  return useQuery<PreconPage, ApiError>({
    queryKey: ['precons', game, opts.query, opts.set, opts.type, opts.sort, opts.page],
    queryFn: ({ signal }) =>
      listPrecons(
        game.value,
        {
          q: opts.query.value || undefined,
          set: opts.set.value || undefined,
          type: opts.type.value || undefined,
          sort: opts.sort.value || undefined,
          page: opts.page.value,
          pageSize: PRECON_PAGE_SIZE,
        },
        signal,
      ),
    placeholderData: keepPreviousData,
    // Precons turn over only when the daily sealed sync runs.
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}

/** The filter vocabulary (types + sets that have precons). One request per game. */
export function usePreconFacetsQuery(game: Ref<string>) {
  return useQuery<{ data: PreconFacets }, ApiError>({
    queryKey: ['precon-facets', game],
    queryFn: ({ signal }) => getPreconFacets(game.value, signal),
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}

/** One precon in full, keyed on `['precon', game, slug]`. */
export function usePreconQuery(game: Ref<string>, slug: Ref<string>) {
  return useQuery<PreconDeckDetail, ApiError>({
    queryKey: ['precon', game, slug],
    queryFn: () => getPrecon(game.value, slug.value),
    retry: false,
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}

interface CopyPreconVars {
  game: string
  slug: string
}

/** Copy a precon into the caller's own decks. The new deck is a normal deck, so this
 *  refreshes that game's deck family off the returned detail. */
export function useCopyPreconMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: CopyPreconVars) => copyPrecon(token, vars.game, vars.slug),
    onSettled: (d: DeckDetail | undefined, _e: ApiError | null, vars: CopyPreconVars) =>
      invalidateDeck(qc, d?.game ?? vars.game),
  }
  return useAuthedMutation<DeckDetail, CopyPreconVars>(options)
}
