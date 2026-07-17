import { computed, ref, type Ref } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import {
  getPublicCollection,
  getPublicCollectionDrops,
  getPublicCollectionSets,
  getPublicCollectionSubtypes,
  getPublicCollectionSummary,
  getPublicOwnedCounts,
  getPublicProfile,
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
  PublicProfile,
} from '@/lib/api'
import { CARD_PAGE_SIZE, DROP_PAGE_SIZE, SUBTYPE_PAGE_SIZE } from '@/composables/useCatalog'
import { COLLECTION_DEFAULT_SORT, toSortParam } from '@/lib/cardSort'
import { useBulkThresholdStore } from '@/stores/bulkThreshold'

// Read-only public collection queries (issues #361/#362). These are the unauthenticated
// mirror of `useCollection`: PLAIN `useQuery` (no token), keyed by the reactive handle/game
// so a navigation refetches. A 404 (unknown handle, no public games, or a private game) is
// terminal — `retry: false` surfaces it straight to the view's not-found state.
//
// The browse view (`PublicCollectionBrowseView`) drives the shared `useHoldingsBrowse`
// engine by binding the handle into these hooks via closures — the same pattern the landing
// view uses for `useHoldingsLanding` — so their signatures line up with the engine's surface
// (`useListQuery`/`useDropsQuery`/`useSubtypesQuery`/`useSummaryQuery`/`useCounts`).

/** A user's public profile: identity + a summary per game they've made public. */
export function usePublicProfileQuery(handle: Ref<string>) {
  return useQuery<PublicProfile, ApiError>({
    queryKey: ['public-profile', handle],
    queryFn: () => getPublicProfile(handle.value),
    retry: false,
  })
}

/** Aggregate stats for a user's public collection in a game, optionally scoped to one set
 * (and, with `includeRelated`, that set's whole group — so the browse view's scoped
 * value/completion line matches the authed one). The scope params default to the
 * whole-collection scope, so the landing's `(handle, game)` callers keep their exact shape.
 *
 * The bulk-value split follows the VIEWER's own bulk-threshold preference (the same
 * client-side setting the authed collection summary uses), so a signed-in visitor sees a
 * public collection's bulk subtotal through their own cutoff instead of a fixed $1. It's in
 * the query key (as a computed ref) so changing it in Settings refetches; the store default
 * matches the server's, so a never-changed / signed-out viewer gets the standard $1 split. */
export function usePublicCollectionSummaryQuery(
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
    queryKey: ['public-summary', handle, game, setCode, includeRelated, bulkMaxCents],
    queryFn: () =>
      getPublicCollectionSummary(handle.value, game.value, {
        set: setCode.value || undefined,
        includeRelated: includeRelated.value,
        bulkMaxCents: bulkMaxCents.value,
      }),
    retry: false,
  })
}

/** The sets the user owns cards in for a public game — the per-set landing tiles. */
export function usePublicCollectionSetsQuery(handle: Ref<string>, game: Ref<string>) {
  return useQuery<{ data: CollectionSet[] }, ApiError>({
    queryKey: ['public-sets', handle, game],
    queryFn: () => getPublicCollectionSets(handle.value, game.value),
    retry: false,
  })
}

/** A page of a user's public collection for a game, optionally scoped to one set.
 * `page`/`query`/`sort` (and `set`) are reactive (carried in the key so a change
 * refetches); the previous page stays visible while the next loads. `opts.enabled` lets
 * the browse engine idle this held-only query in its show-ghosts / by-drop modes. */
export function usePublicCollectionQuery(
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
    queryKey: ['public-collection', handle, game, setCode, related, query, sort, page],
    queryFn: () =>
      getPublicCollection(handle.value, game.value, {
        page: page.value,
        pageSize: CARD_PAGE_SIZE,
        q: query.value || undefined,
        set: setCode.value || undefined,
        // With a set scope, `?related=1` spans the set's whole group (root + related
        // sub-sets) — the backend honours include_related on the public list too.
        includeRelated: related.value || undefined,
        ...toSortParam(sort.value, COLLECTION_DEFAULT_SORT),
      }),
    placeholderData: keepPreviousData,
    enabled: opts.enabled,
    retry: false,
  })
}

/** A page (by Secret Lair drop) of a user's public collection in a drop-grouped set —
 * the public mirror of the collection's by-drop view. Gated on the by-drop mode via
 * `opts.enabled`; `query` narrows the held cards within each drop. */
export function usePublicCollectionDropsQuery(
  handle: Ref<string>,
  game: Ref<string>,
  code: Ref<string>,
  page: Ref<number>,
  query: Ref<string>,
  opts: { enabled?: Ref<boolean> } = {},
) {
  return useQuery<CollectionDropGroupPage, ApiError>({
    queryKey: ['public-drops', handle, game, code, query, page],
    queryFn: () =>
      getPublicCollectionDrops(handle.value, game.value, code.value, {
        page: page.value,
        pageSize: DROP_PAGE_SIZE,
        q: query.value || undefined,
      }),
    placeholderData: keepPreviousData,
    enabled: opts.enabled,
    retry: false,
  })
}

/** A page (by card sub-type / treatment) of a user's public collection in a set — the
 * public mirror of the collection's by-treatment view. Gated via `opts.enabled`. */
export function usePublicCollectionSubtypesQuery(
  handle: Ref<string>,
  game: Ref<string>,
  code: Ref<string>,
  page: Ref<number>,
  query: Ref<string>,
  opts: { enabled?: Ref<boolean> } = {},
) {
  return useQuery<CollectionSubtypeGroupPage, ApiError>({
    queryKey: ['public-subtypes', handle, game, code, query, page],
    queryFn: () =>
      getPublicCollectionSubtypes(handle.value, game.value, code.value, {
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
 * Which of the currently-browsed catalog cards the owner holds, keyed by external id (cards
 * they don't own are absent) — the show-ghosts overlay on the public browse grid. The
 * token-less mirror of `useOwnedCounts`: PLAIN `useQuery` (no auth gating), keyed by the
 * order-independent id set so two renders of the same page dedupe. `ready` reports whether
 * the map reflects the *current* ids (true when there's nothing to look up, or once a
 * non-placeholder result has settled) so the grid's ghost dimming doesn't flash before the
 * counts land — mirroring `useBatchCounts`.
 */
export function usePublicOwnedCounts(handle: Ref<string>, game: Ref<string>, cards: Ref<Card[]>) {
  const cardIds = computed(() => cards.value.map((card) => card.id))
  const idsKey = computed(() => [...cardIds.value].sort().join(','))
  const query = useQuery<OwnedCountsMap, ApiError>({
    queryKey: ['public-owned', handle, game, idsKey],
    queryFn: () => getPublicOwnedCounts(handle.value, game.value, cardIds.value),
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
