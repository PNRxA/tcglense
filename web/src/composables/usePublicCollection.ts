import type { Ref } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { getPublicCollection, getPublicCollectionSummary, getPublicProfile } from '@/lib/api'
import type { ApiError, CollectionPage, CollectionSummary, PublicProfile } from '@/lib/api'
import { CARD_PAGE_SIZE } from '@/composables/useCatalog'
import { COLLECTION_DEFAULT_SORT, toSortParam } from '@/lib/cardSort'

// Read-only public collection queries (issues #361/#362). These are the unauthenticated
// mirror of `useCollection`: PLAIN `useQuery` (no token), keyed by the reactive handle/game
// so a navigation refetches. A 404 (unknown handle, no public games, or a private game) is
// terminal — `retry: false` surfaces it straight to the view's not-found state.

/** A user's public profile: identity + a summary per game they've made public. */
export function usePublicProfileQuery(handle: Ref<string>) {
  return useQuery<PublicProfile, ApiError>({
    queryKey: ['public-profile', handle],
    queryFn: () => getPublicProfile(handle.value),
    retry: false,
  })
}

/** Aggregate stats for a user's public collection in a game. */
export function usePublicCollectionSummaryQuery(handle: Ref<string>, game: Ref<string>) {
  return useQuery<CollectionSummary, ApiError>({
    queryKey: ['public-summary', handle, game],
    queryFn: () => getPublicCollectionSummary(handle.value, game.value),
    retry: false,
  })
}

/** A page of a user's public collection for a game. `page`/`query`/`sort` are reactive
 * (carried in the key so a change refetches); the previous page stays visible while the
 * next loads. */
export function usePublicCollectionQuery(
  handle: Ref<string>,
  game: Ref<string>,
  page: Ref<number>,
  query: Ref<string>,
  sort: Ref<string>,
) {
  return useQuery<CollectionPage, ApiError>({
    queryKey: ['public-collection', handle, game, query, sort, page],
    queryFn: () =>
      getPublicCollection(handle.value, game.value, {
        page: page.value,
        pageSize: CARD_PAGE_SIZE,
        q: query.value || undefined,
        ...toSortParam(sort.value, COLLECTION_DEFAULT_SORT),
      }),
    placeholderData: keepPreviousData,
    retry: false,
  })
}
