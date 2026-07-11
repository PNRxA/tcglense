import { listQuery, request } from './client'
import type {
  CollectionDropGroupPage,
  CollectionDropsParams,
  CollectionListParams,
  CollectionPage,
  CollectionSubtypeGroupPage,
  OwnedCountsMap,
} from './collection'
import type { CollectionQuantities, CollectionSet, CollectionSummary } from './generated'

// ---------- Shared holdings API factory ----------
//
// Collections and the wish list are independent tables that share the exact same
// holding shape and endpoint layout (the backend reuses the same DTOs), differing only
// by the URL base segment (`/api/collection` vs `/api/wishlist`) and the batch-counts
// leaf (`/owned` vs `/counts`). Rather than keep two near-identical client modules, the
// shared, base-parameterized functions live here; `collection.ts` and `wishlist.ts`
// each instantiate this factory and re-export its members under their existing names.
//
// Every call takes an access `token` (obtained via the auth store's `authFetch`, which
// the `useAuthed*` composables wire up). Card ids are the same external ids the public
// catalog exposes. Paths are built here so they can be unit-tested.

/**
 * Max ids per batch-counts request. Kept safely under the server's 500-id cap so we can
 * split an arbitrarily large page (e.g. a drop-grouped set whose by-drop page flattens
 * to a big trailing "Other" group) into batches rather than tripping the cap — which
 * would 422 and silently drop every badge on the page.
 */
const COUNTS_BATCH_SIZE = 400

/** Build the base-parameterized holdings client for one holding table. */
export function makeHoldingApi(base: 'collection' | 'wishlist', countsLeaf: 'owned' | 'counts') {
  /** Relative `/api/{base}/...` path for a user's holdings in a game. */
  const path = (game: string, params: CollectionListParams = {}): string =>
    `/api/${base}/${encodeURIComponent(game)}${listQuery(params)}`

  /** Relative `/api/{base}/{game}/cards/{id}` path for one card's holding. */
  const entryPath = (game: string, id: string): string =>
    `/api/${base}/${encodeURIComponent(game)}/cards/${encodeURIComponent(id)}`

  /** The signed-in user's held cards for a game, most-recently-updated first. */
  const list = (
    token: string,
    game: string,
    params?: CollectionListParams,
  ): Promise<CollectionPage> => request<CollectionPage>(path(game, params), { token })

  /** Aggregate stats (unique cards, total copies, estimated value) for the holdings,
   * optionally scoped to a single set. With a `set` and `includeRelated`, the stats span
   * the set's whole group (root + related sub-sets). `bulkMaxCents` (the collection's
   * bulk-threshold preference, in cents) sets the cutoff the server splits the bulk
   * subtotal at; omitted = the server default. */
  const summary = (
    token: string,
    game: string,
    set?: string,
    includeRelated?: boolean,
    bulkMaxCents?: number,
  ): Promise<CollectionSummary> => {
    // include_related only means anything alongside a set scope (matches the backend).
    const qs = listQuery({ set, includeRelated: set ? includeRelated : undefined, bulkMaxCents })
    return request<CollectionSummary>(`/api/${base}/${encodeURIComponent(game)}/summary${qs}`, {
      token,
    })
  }

  /** The sets the user holds cards in, newest set first — the per-set landing.
   * `bulkMaxCents` sets each tile's bulk cutoff, matching the summary header. */
  const sets = (
    token: string,
    game: string,
    bulkMaxCents?: number,
  ): Promise<{ data: CollectionSet[] }> => {
    const qs = listQuery({ bulkMaxCents })
    return request<{ data: CollectionSet[] }>(
      `/api/${base}/${encodeURIComponent(game)}/sets${qs}`,
      {
        token,
      },
    )
  }

  /** Relative `/api/{base}/{game}/sets/{code}/drops` path (paginated by drop). */
  const setDropsPath = (game: string, code: string, params: CollectionDropsParams = {}): string => {
    const g = encodeURIComponent(game)
    const c = encodeURIComponent(code)
    return `/api/${base}/${g}/sets/${c}/drops${listQuery(params)}`
  }

  /** The signed-in user's held cards in a drop-grouped set (e.g. Secret Lair), grouped by
   * Secret Lair drop and paginated by drop. Only valid where `has_drops` is true. */
  const getSetDrops = (
    token: string,
    game: string,
    code: string,
    params?: CollectionDropsParams,
  ): Promise<CollectionDropGroupPage> =>
    request<CollectionDropGroupPage>(setDropsPath(game, code, params), { token })

  /** Relative `/api/{base}/{game}/sets/{code}/subtypes` path (paginated by sub-type). */
  const setSubtypesPath = (
    game: string,
    code: string,
    params: CollectionDropsParams = {},
  ): string => {
    const g = encodeURIComponent(game)
    const c = encodeURIComponent(code)
    return `/api/${base}/${g}/sets/${c}/subtypes${listQuery(params)}`
  }

  /** The signed-in user's held cards in a set, grouped by card sub-type (treatment) and
   * paginated by sub-type. Offered where the tile's `has_subtypes` is true. */
  const getSetSubtypes = (
    token: string,
    game: string,
    code: string,
    params?: CollectionDropsParams,
  ): Promise<CollectionSubtypeGroupPage> =>
    request<CollectionSubtypeGroupPage>(setSubtypesPath(game, code, params), { token })

  /**
   * Held counts for the given card ids that the user holds, keyed by external id (cards
   * they don't hold are simply absent). Sent as a POST rather than a GET query so a big
   * browse page's id list can't blow the request-line length behind a proxy, and split
   * into batches under the server's id cap so any page size works; the batch maps are
   * merged (batches are disjoint slices, so there's nothing to reconcile). The leaf is
   * `/owned` for a collection and `/counts` for the wish list.
   */
  const counts = async (token: string, game: string, ids: string[]): Promise<OwnedCountsMap> => {
    if (ids.length === 0) return {}
    const endpoint = `/api/${base}/${encodeURIComponent(game)}/${countsLeaf}`
    const batches: string[][] = []
    for (let i = 0; i < ids.length; i += COUNTS_BATCH_SIZE) {
      batches.push(ids.slice(i, i + COUNTS_BATCH_SIZE))
    }
    const responses = await Promise.all(
      batches.map((batch) =>
        request<{ data: OwnedCountsMap }>(endpoint, {
          method: 'POST',
          body: { ids: batch },
          token,
        }),
      ),
    )
    return Object.assign({}, ...responses.map((response) => response.data))
  }

  /** How many copies of one card the user holds (zeros when not held). */
  const getEntry = (token: string, game: string, id: string): Promise<CollectionQuantities> =>
    request<CollectionQuantities>(entryPath(game, id), { token })

  /** Set the held counts for one card (absolute, not a delta). Both zero removes it. */
  const setEntry = (
    token: string,
    game: string,
    id: string,
    body: CollectionQuantities,
  ): Promise<CollectionQuantities> =>
    request<CollectionQuantities>(entryPath(game, id), { method: 'PUT', body, token })

  return {
    path,
    entryPath,
    list,
    summary,
    sets,
    setDropsPath,
    getSetDrops,
    setSubtypesPath,
    getSetSubtypes,
    counts,
    getEntry,
    setEntry,
  }
}
