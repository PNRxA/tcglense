import { computed, ref, type Ref } from 'vue'
import { keepPreviousData, useQueryClient, type QueryClient } from '@tanstack/vue-query'
import type {
  ApiError,
  Card,
  CollectionDropGroupPage,
  CollectionDropsParams,
  CollectionListParams,
  CollectionPage,
  CollectionQuantities,
  CollectionSet,
  CollectionSubtypeGroupPage,
  CollectionSummary,
  OwnedCountsMap,
} from '@/lib/api'
import { CARD_PAGE_SIZE, DROP_PAGE_SIZE, SUBTYPE_PAGE_SIZE } from '@/composables/useCatalog'
import { COLLECTION_DEFAULT_SORT, toSortParam } from '@/lib/cardSort'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'
import { useAuthStore } from '@/stores/auth'
import { useBulkThresholdStore } from '@/stores/bulkThreshold'

// ---------- Shared holding query composables ----------
//
// Collections and the wish list are independent tables that share the exact same holding
// shape and the exact same six read hooks (list, drops, subtypes, summary, sets, entry),
// the browse-badge counts hook, the invalidation helper, and the set-entry mutation. The
// two families differ only by: the query-key prefix (`'collection'` vs `'wishlist'` — the
// prefixes are load-bearing both for cache isolation and for `useAuthCacheReset`'s
// wholesale wipe), the underlying api functions, the batch-counts key suffix
// (`collection-owned` vs `wishlist-counts`), the collection threading its bulk-threshold
// preference into the summary/sets keys+calls, and the collection invalidating an extra
// `collection-value-history` key. `useCollection.ts` and `useWishlist.ts` each instantiate
// this factory and re-export its members under their existing names/signatures.
//
// The option objects are built as intermediate variables rather than inline literals on
// purpose: TanStack's deeply-reactive option types make an inline literal trip
// excess-property checks through the `useAuthed*` wrappers (see `lib/queries.ts`), so a
// plain variable (with explicit callback param types) is the clean way to pass them.

/** Variables for a holding write: which card, and the desired absolute counts. */
export interface SetHoldingVars {
  game: string
  id: string
  quantity: number
  foil_quantity: number
}

/** The per-holding configuration a factory instance is built from. */
export interface HoldingQueriesConfig {
  /** Query-key prefix — `'collection'` or `'wishlist'`. Every key family derives from it. */
  prefix: 'collection' | 'wishlist'
  /** Batch-counts key suffix: `'collection-owned'` for a collection, `'wishlist-counts'`
   * for the wish list (the leaf differs, so this is not `${prefix}-…`). */
  countsKey: string
  getList: (token: string, game: string, params?: CollectionListParams) => Promise<CollectionPage>
  getSetDrops: (
    token: string,
    game: string,
    code: string,
    params?: CollectionDropsParams,
  ) => Promise<CollectionDropGroupPage>
  getSetSubtypes: (
    token: string,
    game: string,
    code: string,
    params?: CollectionDropsParams,
  ) => Promise<CollectionSubtypeGroupPage>
  getSummary: (
    token: string,
    game: string,
    set?: string,
    includeRelated?: boolean,
    bulkMaxCents?: number,
  ) => Promise<CollectionSummary>
  getSets: (
    token: string,
    game: string,
    bulkMaxCents?: number,
  ) => Promise<{ data: CollectionSet[] }>
  getEntry: (token: string, game: string, id: string) => Promise<CollectionQuantities>
  getCounts: (token: string, game: string, ids: string[]) => Promise<OwnedCountsMap>
  setEntry: (
    token: string,
    game: string,
    id: string,
    body: CollectionQuantities,
  ) => Promise<CollectionQuantities>
  /** Collection only: thread the bulk-threshold preference into summary/sets keys+calls. */
  withBulkThreshold: boolean
  /** Collection only: also invalidate the `collection-value-history` key on a write. */
  invalidateValueHistory: boolean
}

/**
 * Held counts for the items currently being browsed, keyed by external id (only held
 * items are present) — the data behind the count badges overlaid on the public browse
 * grids (issue #85). Disabled while signed out (badges are a signed-in feature) and when
 * there are no items to look up; the query key carries the id set so a new page refetches
 * while an identical set dedupes. Returns an empty map while signed out so badges clear
 * immediately on logout regardless of any lingering cache. Generic over anything with an
 * `id` (cards or sealed products): the card factory delegates its `useCounts` here, and
 * the wish list's sealed-product counts hook calls it directly.
 *
 * `ready` reports whether the map actually reflects the *current* items: it's true when
 * signed out or there's nothing to look up (a `{}` map is authoritative then), or once
 * the query has settled a non-placeholder result for this id set. The show-ghosts view
 * (issue #112) gates its dimming on it so held cards don't flash as ghosts in the window
 * before their counts load (an empty map would otherwise read as "everything unheld").
 */
export function useBatchCounts(
  countsKey: string,
  getCounts: (token: string, game: string, ids: string[]) => Promise<OwnedCountsMap>,
  game: Ref<string>,
  items: Ref<{ id: string }[]>,
  opts: { enabled?: Ref<boolean>; staleTime?: number } = {},
) {
  const auth = useAuthStore()
  const cardIds = computed(() => items.value.map((item) => item.id))
  // A stable, order-independent key: two renders of the same page hit the same cache.
  const idsKey = computed(() => [...cardIds.value].sort().join(','))
  const options = {
    queryKey: [countsKey, game, idsKey],
    queryFn: (token: string) => getCounts(token, game.value, cardIds.value),
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

/**
 * Build the shared holding query composables for one holding table (collection or wish
 * list). Reads go through `useAuthed*` (which routes via the auth store's `authFetch`,
 * refreshing an expired access token transparently); writes invalidate the dependent
 * reads so the list, summary, and per-card controls stay in sync after an edit.
 */
export function makeHoldingQueries(cfg: HoldingQueriesConfig) {
  const { prefix } = cfg

  /**
   * Refresh every view that depends on the holding's contents after a write — a per-card
   * edit or (for a collection) a completed import/sync. Covers the grid, the summary
   * header, the per-card count steppers, the by-drop view, the per-set landing tiles, and
   * the browse-grid count badges. Pass `entryId` to scope the per-card entry invalidation
   * to the edited card; an import touches many cards, so it invalidates the whole game.
   */
  function invalidate(qc: QueryClient, game: string, opts?: { entryId?: string }) {
    qc.invalidateQueries({ queryKey: [prefix, game] })
    qc.invalidateQueries({ queryKey: [`${prefix}-summary`, game] })
    if (cfg.invalidateValueHistory) {
      qc.invalidateQueries({ queryKey: ['collection-value-history', game] })
    }
    qc.invalidateQueries({
      queryKey: opts?.entryId ? [`${prefix}-entry`, game, opts.entryId] : [`${prefix}-entry`, game],
    })
    qc.invalidateQueries({ queryKey: [`${prefix}-drops`, game] })
    qc.invalidateQueries({ queryKey: [`${prefix}-sets`, game] })
    qc.invalidateQueries({ queryKey: [cfg.countsKey, game] })
  }

  /** A page of the user's held cards for a game. `page`, `query` and `sort` are reactive:
   * `query` is a Scryfall-style search (same syntax as the catalog) and `sort` is a
   * `field:dir` value (see `lib/cardSort`), all carried in the query key so a change
   * refetches. An optional `set` ref scopes the list to one set, ANDed with the search.
   * `useAuthedQuery` disables it while signed out. */
  function useListQuery(
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
      queryKey: [prefix, game, setCode, query, sort, page, includeRelated],
      queryFn: (token: string) =>
        cfg.getList(token, game.value, {
          page: page.value,
          // The catalog grids' page size, so the holding grid matches them.
          pageSize: CARD_PAGE_SIZE,
          q: query.value || undefined,
          set: setCode.value || undefined,
          includeRelated: includeRelated.value || undefined,
          ...toSortParam(sort.value, COLLECTION_DEFAULT_SORT),
        }),
      // Keep the current grid visible while the next page loads (smoother paging).
      placeholderData: keepPreviousData,
      // Off when a caller opts out — the show-ghosts view fetches the full catalog instead,
      // and the by-drop view fetches drops, so the held-only flat query stays idle in both
      // (no throwaway fetch a drop-set/ghost link would discard).
      enabled: opts.enabled,
    }
    return useAuthedQuery<CollectionPage>(options)
  }

  /** A page (by drop) of the signed-in user's held cards in a drop-grouped set (e.g.
   * Secret Lair), grouped by Secret Lair drop — the holding mirror of the catalog's
   * by-drop view. `code` is the set; `page`/`query` are reactive (carried in the key). The
   * caller gates it on the by-drop view being active (and auth) via `opts.enabled`. */
  function useDropsQuery(
    game: Ref<string>,
    code: Ref<string>,
    page: Ref<number>,
    query: Ref<string>,
    opts: { enabled?: Ref<boolean> } = {},
  ) {
    const options = {
      queryKey: [`${prefix}-drops`, game, code, query, page],
      queryFn: (token: string) =>
        cfg.getSetDrops(token, game.value, code.value, {
          page: page.value,
          pageSize: DROP_PAGE_SIZE,
          q: query.value || undefined,
        }),
      placeholderData: keepPreviousData,
      enabled: opts.enabled,
    }
    return useAuthedQuery<CollectionDropGroupPage>(options)
  }

  /** A page (by sub-type) of the signed-in user's held cards in a set, grouped by card
   * treatment. Gated on the by-treatment view via `opts.enabled`; `query` narrows the held
   * cards within each sub-type. */
  function useSubtypesQuery(
    game: Ref<string>,
    code: Ref<string>,
    page: Ref<number>,
    query: Ref<string>,
    opts: { enabled?: Ref<boolean> } = {},
  ) {
    const options = {
      queryKey: [`${prefix}-subtypes`, game, code, query, page],
      queryFn: (token: string) =>
        cfg.getSetSubtypes(token, game.value, code.value, {
          page: page.value,
          pageSize: SUBTYPE_PAGE_SIZE,
          q: query.value || undefined,
        }),
      placeholderData: keepPreviousData,
      enabled: opts.enabled,
    }
    return useAuthedQuery<CollectionSubtypeGroupPage>(options)
  }

  /** Aggregate stats (unique cards, total copies, estimated value) for the holding,
   * optionally scoped to one set — and, with `includeRelated`, that set's whole group (so
   * the value matches the include-related browse view). */
  function useSummaryQuery(
    game: Ref<string>,
    set?: Ref<string | undefined>,
    opts: { enabled?: Ref<boolean>; includeRelated?: Ref<boolean> } = {},
  ) {
    const setCode = set ?? ref<string | undefined>(undefined)
    const includeRelated = opts.includeRelated ?? ref(false)
    if (cfg.withBulkThreshold) {
      // The user's bulk-threshold preference decides the cutoff the server splits the bulk
      // value at. It's in the query key (as a computed ref) so changing it in Settings
      // refetches the summary; the store default matches the server's, so a signed-out /
      // never-changed user gets the standard $1 split.
      const bulkThreshold = useBulkThresholdStore()
      const bulkMaxCents = computed(() => bulkThreshold.cents)
      const options = {
        queryKey: [`${prefix}-summary`, game, setCode, includeRelated, bulkMaxCents],
        queryFn: (token: string) =>
          cfg.getSummary(
            token,
            game.value,
            setCode.value || undefined,
            includeRelated.value,
            bulkMaxCents.value,
          ),
        enabled: opts.enabled,
      }
      return useAuthedQuery<CollectionSummary>(options)
    }
    const options = {
      queryKey: [`${prefix}-summary`, game, setCode, includeRelated],
      queryFn: (token: string) =>
        cfg.getSummary(token, game.value, setCode.value || undefined, includeRelated.value),
      enabled: opts.enabled,
    }
    return useAuthedQuery<CollectionSummary>(options)
  }

  /** The sets the signed-in user holds cards in (newest set first) — the per-set landing.
   * For a collection, carries the bulk-threshold preference so each tile's bulk slice
   * matches the summary header (and refetches when the threshold changes). */
  function useSetsQuery(game: Ref<string>) {
    if (cfg.withBulkThreshold) {
      const bulkThreshold = useBulkThresholdStore()
      const bulkMaxCents = computed(() => bulkThreshold.cents)
      const options = {
        queryKey: [`${prefix}-sets`, game, bulkMaxCents],
        queryFn: (token: string) => cfg.getSets(token, game.value, bulkMaxCents.value),
      }
      return useAuthedQuery<{ data: CollectionSet[] }>(options)
    }
    const options = {
      queryKey: [`${prefix}-sets`, game],
      queryFn: (token: string) => cfg.getSets(token, game.value),
    }
    return useAuthedQuery<{ data: CollectionSet[] }>(options)
  }

  /**
   * How many copies of one card the signed-in user holds — for the card-detail controls.
   * Options let a caller defer and refresh the fetch: `enabled` gates it (e.g. the grid
   * quick-add control only wants the authoritative holding once its popover opens, not for
   * every visible tile), and `staleTime` (e.g. `0`) forces a re-fetch each time the query
   * re-enables so the control never seeds an absolute-count edit off a stale cached holding.
   */
  function useEntryQuery(
    game: Ref<string>,
    id: Ref<string>,
    opts: { enabled?: Ref<boolean>; staleTime?: number } = {},
  ) {
    const options = {
      queryKey: [`${prefix}-entry`, game, id],
      queryFn: (token: string) => cfg.getEntry(token, game.value, id.value),
      enabled: opts.enabled,
      staleTime: opts.staleTime,
    }
    return useAuthedQuery<CollectionQuantities>(options)
  }

  /** Held counts for the cards currently being browsed — the browse-grid count badges
   * (issue #85). Delegates to the standalone `useBatchCounts`, threading this factory's
   * counts key + api function; see that hook for the ready/fetching seed-gate semantics. */
  function useCounts(
    game: Ref<string>,
    cards: Ref<Card[]>,
    opts: { enabled?: Ref<boolean>; staleTime?: number } = {},
  ) {
    return useBatchCounts(cfg.countsKey, cfg.getCounts, game, cards, opts)
  }

  /**
   * Set the held counts for a card. On success the per-card cache is updated immediately;
   * on settle the list + summary (+ that card's entry) are invalidated so every dependent
   * view refreshes.
   */
  function useSetEntryMutation() {
    const qc = useQueryClient()
    const options = {
      mutationFn: (token: string, vars: SetHoldingVars) =>
        cfg.setEntry(token, vars.game, vars.id, {
          quantity: vars.quantity,
          foil_quantity: vars.foil_quantity,
        }),
      onSuccess: (data: CollectionQuantities, vars: SetHoldingVars) => {
        qc.setQueryData([`${prefix}-entry`, vars.game, vars.id], data)
      },
      onSettled: (
        _data: CollectionQuantities | undefined,
        _error: ApiError | null,
        vars: SetHoldingVars,
      ) => {
        invalidate(qc, vars.game, { entryId: vars.id })
      },
    }
    return useAuthedMutation<CollectionQuantities, SetHoldingVars>(options)
  }

  return {
    invalidate,
    useListQuery,
    useDropsQuery,
    useSubtypesQuery,
    useSummaryQuery,
    useSetsQuery,
    useEntryQuery,
    useCounts,
    useSetEntryMutation,
  }
}
