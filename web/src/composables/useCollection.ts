import { computed, type Ref } from 'vue'
import { keepPreviousData, useQueryClient, type QueryClient } from '@tanstack/vue-query'
import {
  deleteCollectionSource,
  getCollection,
  getCollectionEntry,
  getCollectionOwned,
  getCollectionSource,
  getCollectionSummary,
  getImportJob,
  importCollection,
  importCollectionCsv,
  saveCollectionSource,
  setCollectionEntry,
  syncCollectionSource,
  type ApiError,
  type Card,
  type CollectionPage,
  type CollectionProvider,
  type CollectionQuantities,
  type CollectionSource,
  type CollectionSummary,
  type ImportJob,
  type ImportSummary,
  type OwnedCountsMap,
  type ReconcileMode,
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

/** A page of the user's owned cards for a game. `page`, `query` and `sort` are
 * reactive: `query` is a Scryfall-style search (same syntax as the catalog) and
 * `sort` is a `field:dir` value (see `lib/cardSort`), both carried in the query key
 * so a change refetches. Disabled while signed out — the collection routes are
 * public, so a signed-out visitor lands here (and is prompted to sign in) without
 * triggering an auth call. */
export function useCollectionQuery(
  game: Ref<string>,
  page: Ref<number>,
  query: Ref<string>,
  sort: Ref<string>,
) {
  const auth = useAuthStore()
  const options = {
    queryKey: ['collection', game, query, sort, page],
    queryFn: (token: string) =>
      getCollection(token, game.value, {
        page: page.value,
        pageSize: COLLECTION_PAGE_SIZE,
        q: query.value || undefined,
        ...toSortParam(sort.value, COLLECTION_DEFAULT_SORT),
      }),
    // Keep the current grid visible while the next page loads (smoother paging).
    placeholderData: keepPreviousData,
    enabled: computed(() => auth.isAuthenticated),
  }
  return useAuthedQuery<CollectionPage>(options)
}

/** Aggregate stats (unique cards, total copies, estimated value) for the collection.
 * Disabled while signed out (see `useCollectionQuery`). */
export function useCollectionSummaryQuery(game: Ref<string>) {
  const auth = useAuthStore()
  const options = {
    queryKey: ['collection-summary', game],
    queryFn: (token: string) => getCollectionSummary(token, game.value),
    enabled: computed(() => auth.isAuthenticated),
  }
  return useAuthedQuery<CollectionSummary>(options)
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
 */
export function useOwnedCounts(game: Ref<string>, cards: Ref<Card[]>) {
  const auth = useAuthStore()
  const cardIds = computed(() => cards.value.map((card) => card.id))
  // A stable, order-independent key: two renders of the same page hit the same cache.
  const idsKey = computed(() => [...cardIds.value].sort().join(','))
  const options = {
    queryKey: ['collection-owned', game, idsKey],
    queryFn: (token: string) => getCollectionOwned(token, game.value, cardIds.value),
    enabled: computed(() => auth.isAuthenticated && cardIds.value.length > 0),
    // Keep the previous page's badges up while the next page's counts load.
    placeholderData: keepPreviousData,
  }
  const query = useAuthedQuery<OwnedCountsMap>(options)
  return computed<OwnedCountsMap>(() => (auth.isAuthenticated ? (query.data.value ?? {}) : {}))
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
      // Refresh the browse-grid badges so an edit shows next time a grid is viewed.
      qc.invalidateQueries({ queryKey: ['collection-owned', vars.game] })
    },
  }
  return useAuthedMutation<CollectionQuantities, SetCollectionVars>(options)
}

// ---------- Import / sync from an external collection provider ----------

/**
 * The user's saved external collection link for a game (or null). Drives the
 * "Re-sync" affordance and prefills the import dialog. Disabled while signed out.
 */
export function useCollectionSourceQuery(game: Ref<string>) {
  const auth = useAuthStore()
  const options = {
    queryKey: ['collection-source', game],
    queryFn: (token: string) => getCollectionSource(token, game.value),
    enabled: computed(() => auth.isAuthenticated),
  }
  return useAuthedQuery<CollectionSource | null>(options)
}

/** Variables for a one-off import. */
export interface ImportCollectionVars {
  game: string
  provider: CollectionProvider
  source: string
  mode: ReconcileMode
}

/**
 * Enqueue a one-off import from a provider. Resolves to a job to poll (via
 * {@link useImportJobQuery}); the collection caches are invalidated only once that job
 * completes, so nothing is invalidated here.
 */
export function useImportCollectionMutation() {
  const options = {
    mutationFn: (token: string, vars: ImportCollectionVars) =>
      importCollection(token, vars.game, {
        provider: vars.provider,
        source: vars.source,
        mode: vars.mode,
      }),
  }
  return useAuthedMutation<ImportJob, ImportCollectionVars>(options)
}

/** Variables for a CSV upload import: the file and how to reconcile it. */
export interface ImportCsvVars {
  game: string
  file: File
  mode: ReconcileMode
}

/**
 * Import a collection from an uploaded Archidekt CSV export. Resolves **synchronously**
 * to an {@link ImportSummary} (the CSV needs no upstream fetch, so there's no job to
 * poll); the caller invalidates the collection caches on success.
 */
export function useImportCollectionCsvMutation() {
  const options = {
    mutationFn: (token: string, vars: ImportCsvVars) =>
      importCollectionCsv(token, vars.game, vars.file, vars.mode),
  }
  return useAuthedMutation<ImportSummary, ImportCsvVars>(options)
}

/**
 * Poll a background import/sync job until it reaches a terminal status. Enabled only
 * while `jobId` is set; refetches every 2s while `queued`/`running`, then stops.
 */
export function useImportJobQuery(game: Ref<string>, jobId: Ref<number | null>) {
  const auth = useAuthStore()
  const options = {
    queryKey: ['import-job', game, jobId],
    queryFn: (token: string) => getImportJob(token, game.value, jobId.value as number),
    enabled: computed(() => auth.isAuthenticated && jobId.value != null),
    refetchInterval: (query: { state: { data?: ImportJob } }) => {
      const status = query.state.data?.status
      return status === 'queued' || status === 'running' ? 2000 : false
    },
    // A job's status is inherently fresh; don't serve a stale cached terminal state.
    staleTime: 0,
    gcTime: 0,
  }
  return useAuthedQuery<ImportJob>(options)
}

/** Variables for saving a collection link. */
export interface SaveSourceVars {
  game: string
  provider: CollectionProvider
  source: string
  /** Whether saved re-syncs should use smart (incremental) sync. */
  smart?: boolean
}

/** Save (upsert) the collection link; invalidates the saved-source query. */
export function useSaveCollectionSourceMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: SaveSourceVars) =>
      saveCollectionSource(token, vars.game, {
        provider: vars.provider,
        source: vars.source,
        smart: vars.smart,
      }),
    onSettled: (
      _data: CollectionSource | undefined,
      _error: ApiError | null,
      vars: SaveSourceVars,
    ) => {
      qc.invalidateQueries({ queryKey: ['collection-source', vars.game] })
    },
  }
  return useAuthedMutation<CollectionSource, SaveSourceVars>(options)
}

/** Forget the saved collection link; invalidates the saved-source query. */
export function useDeleteCollectionSourceMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: { game: string }) => deleteCollectionSource(token, vars.game),
    onSettled: (_data: void | undefined, _error: ApiError | null, vars: { game: string }) => {
      qc.invalidateQueries({ queryKey: ['collection-source', vars.game] })
    },
  }
  return useAuthedMutation<void, { game: string }>(options)
}

/**
 * Enqueue a re-sync from the saved link (mirror/replace). Resolves to a job to poll; the
 * collection + saved-source caches are invalidated once that job completes (the caller
 * does this on completion, via {@link invalidateCollectionData} + the source query).
 */
export function useSyncCollectionSourceMutation() {
  const options = {
    mutationFn: (token: string, vars: { game: string }) => syncCollectionSource(token, vars.game),
  }
  return useAuthedMutation<ImportJob, { game: string }>(options)
}
