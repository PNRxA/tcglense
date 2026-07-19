import { computed, ref, type Ref } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import {
  getPublicWishlist,
  getPublicWishlistDrops,
  getPublicWishlistOwnedCounts,
  getPublicWishlistProducts,
  getPublicWishlistProductSets,
  getPublicWishlistProductSummary,
  getPublicWishlistSets,
  getPublicWishlistSubtypes,
  getPublicWishlistSummary,
} from '@/lib/api'
import type {
  ApiError,
  Card,
  CollectionDropGroupPage,
  CollectionPage,
  CollectionSet,
  CollectionSubtypeGroupPage,
  CollectionSummary,
  OwnedCountsMap,
  ProductHoldingPage,
  ProductHoldingSet,
  ProductHoldingSummary,
} from '@/lib/api'
import { CARD_PAGE_SIZE, DROP_PAGE_SIZE, SUBTYPE_PAGE_SIZE } from '@/composables/useCatalog'
import { PRODUCT_HOLDING_PAGE_SIZE } from '@/composables/productHoldingQueries'
import { COLLECTION_DEFAULT_SORT, toSortParam } from '@/lib/cardSort'
import { useBulkThresholdStore } from '@/stores/bulkThreshold'

// Read-only public wish-list queries (issue #493) — the unauthenticated wish-list twin of
// `usePublicCollection`: PLAIN `useQuery` (no token), keyed by the reactive handle/game so a
// navigation refetches, with a distinct `public-wishlist*` key family so nothing collides with
// the public-collection reads. A 404 (unknown handle, no public wish list) is terminal
// (`retry: false`). These drive the shared `useHoldingsLanding` / `useHoldingsBrowse` engines
// via the handle-bound closures the public wish-list views pass, exactly like the collection
// side. The profile query itself is shared (`usePublicProfileQuery` in `usePublicCollection`).

/** Aggregate stats for a user's public wish list in a game, optionally scoped to one set. */
export function usePublicWishlistSummaryQuery(
  handle: Ref<string>,
  game: Ref<string>,
  set?: Ref<string | undefined>,
  opts: { includeRelated?: Ref<boolean> } = {},
) {
  const setCode = set ?? ref<string | undefined>(undefined)
  const includeRelated = opts.includeRelated ?? ref(false)
  const bulkThreshold = useBulkThresholdStore()
  const bulkMaxCents = computed(() => bulkThreshold.cents)
  return useQuery<CollectionSummary, ApiError>({
    queryKey: ['public-wishlist-summary', handle, game, setCode, includeRelated, bulkMaxCents],
    queryFn: () =>
      getPublicWishlistSummary(handle.value, game.value, {
        set: setCode.value || undefined,
        includeRelated: includeRelated.value,
        bulkMaxCents: bulkMaxCents.value,
      }),
    retry: false,
  })
}

/** The sets the user wants cards in for a public wish list — the per-set landing tiles. */
export function usePublicWishlistSetsQuery(handle: Ref<string>, game: Ref<string>) {
  return useQuery<{ data: CollectionSet[] }, ApiError>({
    queryKey: ['public-wishlist-sets', handle, game],
    queryFn: () => getPublicWishlistSets(handle.value, game.value),
    retry: false,
  })
}

/** Aggregate stats for a user's public wanted sealed products in a game. */
export function usePublicWishlistProductSummaryQuery(handle: Ref<string>, game: Ref<string>) {
  return useQuery<ProductHoldingSummary, ApiError>({
    queryKey: ['public-wishlist-product-summary', handle, game],
    queryFn: () => getPublicWishlistProductSummary(handle.value, game.value),
    retry: false,
  })
}

/** The sets a user wants sealed products in for a public wish list — the per-set landing tiles. */
export function usePublicWishlistProductSetsQuery(handle: Ref<string>, game: Ref<string>) {
  return useQuery<{ data: ProductHoldingSet[] }, ApiError>({
    queryKey: ['public-wishlist-product-sets', handle, game],
    queryFn: () => getPublicWishlistProductSets(handle.value, game.value),
    retry: false,
  })
}

/** A page of a user's public wanted sealed products, optionally scoped to one set. */
export function usePublicWishlistProductsQuery(
  handle: Ref<string>,
  game: Ref<string>,
  page: Ref<number>,
  set?: Ref<string | undefined>,
) {
  const setCode = set ?? ref<string | undefined>(undefined)
  return useQuery<ProductHoldingPage, ApiError>({
    queryKey: ['public-wishlist-products', handle, game, setCode, page],
    queryFn: () =>
      getPublicWishlistProducts(handle.value, game.value, {
        page: page.value,
        pageSize: PRODUCT_HOLDING_PAGE_SIZE,
        set: setCode.value || undefined,
      }),
    placeholderData: keepPreviousData,
    retry: false,
  })
}

/** A page of a user's public wish list for a game, optionally scoped to one set. */
export function usePublicWishlistQuery(
  handle: Ref<string>,
  game: Ref<string>,
  page: Ref<number>,
  query: Ref<string>,
  sort: Ref<string>,
  set?: Ref<string | undefined>,
  opts: { includeRelated?: Ref<boolean>; enabled?: Ref<boolean> } = {},
) {
  const setCode = set ?? ref<string | undefined>(undefined)
  const related = opts.includeRelated ?? ref(false)
  return useQuery<CollectionPage, ApiError>({
    queryKey: ['public-wishlist', handle, game, setCode, related, query, sort, page],
    queryFn: () =>
      getPublicWishlist(handle.value, game.value, {
        page: page.value,
        pageSize: CARD_PAGE_SIZE,
        q: query.value || undefined,
        set: setCode.value || undefined,
        includeRelated: related.value || undefined,
        ...toSortParam(sort.value, COLLECTION_DEFAULT_SORT),
      }),
    placeholderData: keepPreviousData,
    enabled: opts.enabled,
    retry: false,
  })
}

/** A page (by Secret Lair drop) of a user's public wish list in a drop-grouped set. */
export function usePublicWishlistDropsQuery(
  handle: Ref<string>,
  game: Ref<string>,
  code: Ref<string>,
  page: Ref<number>,
  query: Ref<string>,
  opts: { enabled?: Ref<boolean> } = {},
) {
  return useQuery<CollectionDropGroupPage, ApiError>({
    queryKey: ['public-wishlist-drops', handle, game, code, query, page],
    queryFn: () =>
      getPublicWishlistDrops(handle.value, game.value, code.value, {
        page: page.value,
        pageSize: DROP_PAGE_SIZE,
        q: query.value || undefined,
      }),
    placeholderData: keepPreviousData,
    enabled: opts.enabled,
    retry: false,
  })
}

/** A page (by card sub-type / treatment) of a user's public wish list in a set. */
export function usePublicWishlistSubtypesQuery(
  handle: Ref<string>,
  game: Ref<string>,
  code: Ref<string>,
  page: Ref<number>,
  query: Ref<string>,
  opts: { enabled?: Ref<boolean> } = {},
) {
  return useQuery<CollectionSubtypeGroupPage, ApiError>({
    queryKey: ['public-wishlist-subtypes', handle, game, code, query, page],
    queryFn: () =>
      getPublicWishlistSubtypes(handle.value, game.value, code.value, {
        page: page.value,
        pageSize: SUBTYPE_PAGE_SIZE,
        q: query.value || undefined,
      }),
    placeholderData: keepPreviousData,
    enabled: opts.enabled,
    retry: false,
  })
}

/**
 * Which of the currently-browsed catalog cards the owner wants, keyed by external id (cards
 * they don't want are absent) — the show-ghosts overlay on the public wish-list browse grid.
 * The token-less mirror of `useWishlistCounts`; `ready` reports whether the map reflects the
 * *current* ids so the grid's ghost dimming doesn't flash before the counts land.
 */
export function usePublicWishlistOwnedCounts(
  handle: Ref<string>,
  game: Ref<string>,
  cards: Ref<Card[]>,
) {
  const cardIds = computed(() => cards.value.map((card) => card.id))
  const idsKey = computed(() => [...cardIds.value].sort().join(','))
  const query = useQuery<OwnedCountsMap, ApiError>({
    queryKey: ['public-wishlist-owned', handle, game, idsKey],
    queryFn: () => getPublicWishlistOwnedCounts(handle.value, game.value, cardIds.value),
    enabled: computed(() => cardIds.value.length > 0),
    placeholderData: keepPreviousData,
    retry: false,
  })
  const ownership = computed<OwnedCountsMap>(() => query.data.value ?? {})
  const ready = computed(
    () => cardIds.value.length === 0 || (query.isSuccess.value && !query.isPlaceholderData.value),
  )
  const fetching = computed(() => query.isFetching.value)
  return { ownership, ready, fetching }
}
