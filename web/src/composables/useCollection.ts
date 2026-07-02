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
import { CARD_PAGE_SIZE, DROP_PAGE_SIZE } from '@/composables/useCatalog'
import { COLLECTION_DEFAULT_SORT, toSortParam } from '@/lib/cardSort'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'
import { useAuthStore } from '@/stores/auth'

/**
 * Refresh every view that depends on the collection contents after any collection write
 * — a per-card edit or a completed import/sync. Covers the grid, the summary header, the
 * per-card owned-count steppers, the by-drop owned-cards view, the per-set landing tiles
 * (ownership per set can change broadly), and the browse-grid owned-count badges (which an
 * import can change broadly too). Pass `entryId` to scope the per-card entry invalidation
 * to the edited card; an import touches many cards, so it invalidates the whole game.
 */
export function invalidateCollectionData(
  qc: QueryClient,
  game: string,
  opts?: { entryId?: string },
) {
  qc.invalidateQueries({ queryKey: ['collection', game] })
  qc.invalidateQueries({ queryKey: ['collection-summary', game] })
  qc.invalidateQueries({
    queryKey: opts?.entryId ? ['collection-entry', game, opts.entryId] : ['collection-entry', game],
  })
  qc.invalidateQueries({ queryKey: ['collection-drops', game] })
  qc.invalidateQueries({ queryKey: ['collection-sets', game] })
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

/** A page of the user's owned cards for a game. `page`, `query` and `sort` are
 * reactive: `query` is a Scryfall-style search (same syntax as the catalog) and
 * `sort` is a `field:dir` value (see `lib/cardSort`), all carried in the query key
 * so a change refetches. An optional `set` ref scopes the list to one set (the per-set
 * collection view), ANDed with the search. `useAuthedQuery` disables it while signed out. */
export function useCollectionQuery(
  game: Ref<string>,
  page: Ref<number>,
  query: Ref<string>,
  sort: Ref<string>,
  set?: Ref<string | undefined>,
  opts: { includeRelated?: Ref<boolean>; enabled?: Ref<boolean> } = {},
) {
  // Fall back to stable "no scope" / "not grouped" refs so the query key is well-formed
  // either way.
  const setCode = set ?? ref<string | undefined>(undefined)
  const includeRelated = opts.includeRelated ?? ref(false)
  const options = {
    queryKey: ['collection', game, setCode, query, sort, page, includeRelated],
    queryFn: (token: string) =>
      getCollection(token, game.value, {
        page: page.value,
        // The catalog grids' page size, so the collection grid matches them.
        pageSize: CARD_PAGE_SIZE,
        q: query.value || undefined,
        set: setCode.value || undefined,
        includeRelated: includeRelated.value || undefined,
        ...toSortParam(sort.value, COLLECTION_DEFAULT_SORT),
      }),
    // Keep the current grid visible while the next page loads (smoother paging).
    placeholderData: keepPreviousData,
    // Off when a caller opts out — the show-ghosts view fetches the full catalog instead,
    // and the by-drop view fetches drops, so the owned-only flat query stays idle in both
    // (no throwaway fetch a drop-set/ghost link would discard).
    enabled: opts.enabled,
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
  const options = {
    queryKey: ['collection-drops', game, code, query, page],
    queryFn: (token: string) =>
      getCollectionSetDrops(token, game.value, code.value, {
        page: page.value,
        pageSize: DROP_PAGE_SIZE,
        q: query.value || undefined,
      }),
    placeholderData: keepPreviousData,
    enabled: opts.enabled,
  }
  return useAuthedQuery<CollectionDropGroupPage>(options)
}

/** Aggregate stats (unique cards, total copies, estimated value) for the collection,
 * optionally scoped to one set — and, with `includeRelated`, that set's whole group (so
 * the value matches the include-related browse view). */
export function useCollectionSummaryQuery(
  game: Ref<string>,
  set?: Ref<string | undefined>,
  opts: { enabled?: Ref<boolean>; includeRelated?: Ref<boolean> } = {},
) {
  const setCode = set ?? ref<string | undefined>(undefined)
  const includeRelated = opts.includeRelated ?? ref(false)
  const options = {
    queryKey: ['collection-summary', game, setCode, includeRelated],
    queryFn: (token: string) =>
      getCollectionSummary(token, game.value, setCode.value || undefined, includeRelated.value),
    enabled: opts.enabled,
  }
  return useAuthedQuery<CollectionSummary>(options)
}

/** The sets the signed-in user owns cards in (newest set first) — the per-set
 * collection landing. */
export function useCollectionSetsQuery(game: Ref<string>) {
  const options = {
    queryKey: ['collection-sets', game],
    queryFn: (token: string) => getCollectionSets(token, game.value),
  }
  return useAuthedQuery<{ data: CollectionSet[] }>(options)
}

/**
 * How many copies of one card the signed-in user owns — for the card-detail
 * controls. Options let a caller defer and refresh the fetch: `enabled` gates it (e.g.
 * the grid quick-add control only wants the authoritative holding once its popover opens,
 * not for every visible tile), and `staleTime` (e.g. `0`) forces a re-fetch each time the
 * query re-enables so the control never seeds an absolute-count edit off a stale cached
 * holding.
 */
export function useCollectionEntryQuery(
  game: Ref<string>,
  id: Ref<string>,
  opts: { enabled?: Ref<boolean>; staleTime?: number } = {},
) {
  const options = {
    queryKey: ['collection-entry', game, id],
    queryFn: (token: string) => getCollectionEntry(token, game.value, id.value),
    enabled: opts.enabled,
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
    enabled: computed(() => cardIds.value.length > 0 && (opts.enabled?.value ?? true)),
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
      invalidateCollectionData(qc, vars.game, { entryId: vars.id })
    },
  }
  return useAuthedMutation<CollectionQuantities, SetCollectionVars>(options)
}
