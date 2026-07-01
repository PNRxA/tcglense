import { computed, ref, type Ref } from 'vue'
import { keepPreviousData, useQueryClient, type QueryClient } from '@tanstack/vue-query'
import {
  getCollection,
  getCollectionEntry,
  getCollectionOwned,
  getCollectionSetDrops,
  getCollectionSets,
  getCollectionSummary,
  setCollectionEntry,
  type ApiError,
  type Card,
  type CollectionDropGroupPage,
  type CollectionPage,
  type CollectionQuantities,
  type CollectionSet,
  type CollectionSummary,
  type OwnedCountsMap,
} from '@/lib/api'
import { COLLECTION_DEFAULT_SORT, toSortParam } from '@/lib/cardSort'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'
import { useAuthStore } from '@/stores/auth'

/** Refresh every view that depends on the collection contents (grid, summary header, and
 * the per-card owned-count steppers). Call after an import/sync job completes. */
export function invalidateCollectionData(qc: QueryClient, game: string) {
  qc.invalidateQueries({ queryKey: ['collection', game] })
  qc.invalidateQueries({ queryKey: ['collection-summary', game] })
  qc.invalidateQueries({ queryKey: ['collection-entry', game] })
  // Refresh the by-drop owned-cards view too.
  qc.invalidateQueries({ queryKey: ['collection-drops', game] })
  // Refresh the per-set landing tiles (ownership per set can change broadly).
  qc.invalidateQueries({ queryKey: ['collection-sets', game] })
  // Refresh the browse-grid owned-count badges too (an import can change ownership broadly).
  qc.invalidateQueries({ queryKey: ['collection-owned', game] })
}

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

/** Drops per page in the by-drop collection view — it paginates over drops (each a
 * handful of owned cards), so it uses a smaller page size than the flat grid (matches
 * the catalog's by-drop view). */
export const COLLECTION_DROP_PAGE_SIZE = 20

/** A page of the user's owned cards for a game. `page`, `query` and `sort` are
 * reactive: `query` is a Scryfall-style search (same syntax as the catalog) and
 * `sort` is a `field:dir` value (see `lib/cardSort`), all carried in the query key
 * so a change refetches. An optional `set` ref scopes the list to one set (the per-set
 * collection view), ANDed with the search. Disabled while signed out — the collection
 * routes are public, so a signed-out visitor lands here (and is prompted to sign in)
 * without triggering an auth call. */
export function useCollectionQuery(
  game: Ref<string>,
  page: Ref<number>,
  query: Ref<string>,
  sort: Ref<string>,
  set?: Ref<string | undefined>,
  opts: { includeRelated?: Ref<boolean>; enabled?: Ref<boolean> } = {},
) {
  const auth = useAuthStore()
  // Fall back to stable "no scope" / "not grouped" refs so the query key is well-formed
  // either way.
  const setCode = set ?? ref<string | undefined>(undefined)
  const includeRelated = opts.includeRelated ?? ref(false)
  const options = {
    queryKey: ['collection', game, setCode, query, sort, page, includeRelated],
    queryFn: (token: string) =>
      getCollection(token, game.value, {
        page: page.value,
        pageSize: COLLECTION_PAGE_SIZE,
        q: query.value || undefined,
        set: setCode.value || undefined,
        includeRelated: includeRelated.value || undefined,
        ...toSortParam(sort.value, COLLECTION_DEFAULT_SORT),
      }),
    // Keep the current grid visible while the next page loads (smoother paging).
    placeholderData: keepPreviousData,
    // Signed-in only, and off when a caller opts out — the show-ghosts view fetches the
    // full catalog instead, and the by-drop view fetches drops, so the owned-only flat
    // query stays idle in both (no throwaway fetch a drop-set/ghost link would discard).
    enabled: computed(() => auth.isAuthenticated && (opts.enabled?.value ?? true)),
  }
  return useAuthedQuery<CollectionPage>(options)
}

/** A page (by drop) of the signed-in user's owned cards in a drop-grouped set (e.g.
 * Secret Lair), grouped by Secret Lair drop — the collection mirror of the catalog's
 * by-drop view. `code` is the set; `page`/`query` are reactive (carried in the key). The
 * caller gates it on the by-drop view being active (and auth) via `opts.enabled`. */
export function useCollectionDropsQuery(
  game: Ref<string>,
  code: Ref<string>,
  page: Ref<number>,
  query: Ref<string>,
  opts: { enabled?: Ref<boolean> } = {},
) {
  const auth = useAuthStore()
  const options = {
    queryKey: ['collection-drops', game, code, query, page],
    queryFn: (token: string) =>
      getCollectionSetDrops(token, game.value, code.value, {
        page: page.value,
        pageSize: COLLECTION_DROP_PAGE_SIZE,
        q: query.value || undefined,
      }),
    placeholderData: keepPreviousData,
    enabled: computed(() => auth.isAuthenticated && (opts.enabled?.value ?? true)),
  }
  return useAuthedQuery<CollectionDropGroupPage>(options)
}

/** Aggregate stats (unique cards, total copies, estimated value) for the collection,
 * optionally scoped to one set — and, with `includeRelated`, that set's whole group (so
 * the value matches the include-related browse view). Disabled while signed out (see
 * `useCollectionQuery`). */
export function useCollectionSummaryQuery(
  game: Ref<string>,
  set?: Ref<string | undefined>,
  opts: { enabled?: Ref<boolean>; includeRelated?: Ref<boolean> } = {},
) {
  const auth = useAuthStore()
  const setCode = set ?? ref<string | undefined>(undefined)
  const includeRelated = opts.includeRelated ?? ref(false)
  const options = {
    queryKey: ['collection-summary', game, setCode, includeRelated],
    queryFn: (token: string) =>
      getCollectionSummary(token, game.value, setCode.value || undefined, includeRelated.value),
    enabled: computed(() => auth.isAuthenticated && (opts.enabled?.value ?? true)),
  }
  return useAuthedQuery<CollectionSummary>(options)
}

/** The sets the signed-in user owns cards in (newest set first) — the per-set
 * collection landing. Disabled while signed out (see `useCollectionQuery`). */
export function useCollectionSetsQuery(game: Ref<string>) {
  const auth = useAuthStore()
  const options = {
    queryKey: ['collection-sets', game],
    queryFn: (token: string) => getCollectionSets(token, game.value),
    enabled: computed(() => auth.isAuthenticated),
  }
  return useAuthedQuery<{ data: CollectionSet[] }>(options)
}

/**
 * How many copies of one card the signed-in user owns — for the card-detail
 * controls. Disabled while signed out (the route is public), so a logged-out
 * visitor never triggers an auth call. Options let a caller defer and refresh the
 * fetch: `enabled` gates it (e.g. the grid quick-add control only wants the
 * authoritative holding once its popover opens, not for every visible tile), and
 * `staleTime` (e.g. `0`) forces a re-fetch each time the query re-enables so the
 * control never seeds an absolute-count edit off a stale cached holding.
 */
export function useCollectionEntryQuery(
  game: Ref<string>,
  id: Ref<string>,
  opts: { enabled?: Ref<boolean>; staleTime?: number } = {},
) {
  const auth = useAuthStore()
  const options = {
    queryKey: ['collection-entry', game, id],
    queryFn: (token: string) => getCollectionEntry(token, game.value, id.value),
    enabled: computed(() => auth.isAuthenticated && (opts.enabled?.value ?? true)),
    staleTime: opts.staleTime,
  }
  return useAuthedQuery<CollectionQuantities>(options)
}

/**
 * Owned counts for the cards currently being browsed, keyed by external card id (only
 * owned cards are present) — the data behind the collection badges overlaid on the
 * public browse grids (issue #85). Disabled while signed out (badges are a signed-in
 * feature) and when there are no cards to look up; the query key carries the id set so
 * a new page refetches while an identical set dedupes. Returns an empty map while
 * signed out so badges clear immediately on logout regardless of any lingering cache.
 *
 * `ready` reports whether the map actually reflects the *current* cards: it's true when
 * signed out or there's nothing to look up (a `{}` map is authoritative then), or once
 * the query has settled a non-placeholder result for this id set. The show-ghosts view
 * (issue #112) gates its dimming on it so owned cards don't flash as ghosts in the window
 * before their counts load (an empty map would otherwise read as "everything unowned").
 */
export function useOwnedCounts(
  game: Ref<string>,
  cards: Ref<Card[]>,
  opts: { enabled?: Ref<boolean>; staleTime?: number } = {},
) {
  const auth = useAuthStore()
  const cardIds = computed(() => cards.value.map((card) => card.id))
  // A stable, order-independent key: two renders of the same page hit the same cache.
  const idsKey = computed(() => [...cardIds.value].sort().join(','))
  const options = {
    queryKey: ['collection-owned', game, idsKey],
    queryFn: (token: string) => getCollectionOwned(token, game.value, cardIds.value),
    enabled: computed(
      () => auth.isAuthenticated && cardIds.value.length > 0 && (opts.enabled?.value ?? true),
    ),
    // Keep the previous page's badges up while the next page's counts load.
    placeholderData: keepPreviousData,
    // A caller can force a fresh authoritative fetch (e.g. the quick-add dialog seeds
    // absolute-count editors from this, so it wants `0` to re-read on each open). Only
    // set the key when asked: a bare `staleTime: undefined` would override the client's
    // 5-minute default down to 0, refetching the badge queries far more than needed.
    ...(opts.staleTime !== undefined ? { staleTime: opts.staleTime } : {}),
  }
  const query = useAuthedQuery<OwnedCountsMap>(options)
  const ownership = computed<OwnedCountsMap>(() =>
    auth.isAuthenticated ? (query.data.value ?? {}) : {},
  )
  const ready = computed(
    () =>
      !auth.isAuthenticated ||
      cardIds.value.length === 0 ||
      (query.isSuccess.value && !query.isPlaceholderData.value),
  )
  // A fetch in flight. A caller seeding *absolute-count* editors must gate on
  // `ready && !fetching`, not `ready` alone: on a same-key refetch (e.g. the quick-add
  // dialog reopening the same name with `staleTime: 0`) `ready` stays true off the
  // retained cache while the fresh data loads, so `ready` by itself would let an edit
  // save off a stale seed (mirrors OwnedCountControl's `isSuccess && !isFetching`).
  const fetching = computed(() => query.isFetching.value)
  return { ownership, ready, fetching }
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
      // Refresh the by-drop owned-cards view so an edit shows there too.
      qc.invalidateQueries({ queryKey: ['collection-drops', vars.game] })
      // Refresh the per-set landing tiles (owned counts per set change on an edit).
      qc.invalidateQueries({ queryKey: ['collection-sets', vars.game] })
      // Refresh the browse-grid badges so an edit shows next time a grid is viewed.
      qc.invalidateQueries({ queryKey: ['collection-owned', vars.game] })
    },
  }
  return useAuthedMutation<CollectionQuantities, SetCollectionVars>(options)
}
