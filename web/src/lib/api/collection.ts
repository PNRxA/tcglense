import { API_URL, ApiError, listQuery, request } from './client'
import type { PriceRange } from './catalog'
import type {
  CollectionDropGroup,
  CollectionEntry,
  CollectionQuantities,
  CollectionSet,
  CollectionSubtypeGroup,
  CollectionSummary,
  CollectionValuePoint,
  Page,
} from './generated'

// ---------- Collections (per-user, authenticated) ----------
//
// Every call takes an access `token` (obtained via the auth store's `authFetch`,
// which the `useAuthed*` composables wire up). Card ids are the same external ids
// the public catalog exposes. Paths are built here so they can be unit-tested; the
// heavy lifting (auth header, credentials, JSON) lives in `request`. The wire types
// are generated from the API's Rust DTOs into `./generated` and re-exported here.

export type {
  CollectionDropGroup,
  CollectionEntry,
  CollectionQuantities,
  CollectionSet,
  CollectionSubtypeGroup,
  CollectionSummary,
} from './generated'

/** Owned counts for a batch of cards, keyed by external card id (owned cards only) —
 * the `data` payload of the wire `DataBody<HashMap<..>>` response. */
export type OwnedCountsMap = Record<string, CollectionQuantities>

/** A page of collection entries plus pagination cursors. */
export type CollectionPage = Page<CollectionEntry>

export interface CollectionListParams {
  page?: number
  pageSize?: number
  /** Scryfall-style search query (same syntax as the catalog card lists). */
  q?: string
  /** Sort key (`updated`/`name`/`rarity`/`released`/`cmc`/`price`). */
  sort?: string
  dir?: 'asc' | 'desc'
  /** Set-code scope: only cards from this set, ANDed with `q`. Absent = every set. */
  set?: string
  /** With a `set` scope, span the set's whole group (root + related sub-sets) instead
   * of just the one set — the collection mirror of the catalog's `include_related`. */
  includeRelated?: boolean
}

/** Relative `/api/collection/...` path for a user's collection in a game. */
export function collectionPath(game: string, params: CollectionListParams = {}): string {
  return `/api/collection/${encodeURIComponent(game)}${listQuery(params)}`
}

/** Relative `/api/collection/{game}/cards/{id}` path for one card's holding. */
export function collectionEntryPath(game: string, id: string): string {
  return `/api/collection/${encodeURIComponent(game)}/cards/${encodeURIComponent(id)}`
}

/** The signed-in user's owned cards for a game, most-recently-updated first. */
export function getCollection(
  token: string,
  game: string,
  params?: CollectionListParams,
): Promise<CollectionPage> {
  return request<CollectionPage>(collectionPath(game, params), { token })
}

/** Aggregate stats (unique cards, total copies, estimated value) for the collection,
 * optionally scoped to a single set (the per-set collection view). With a `set` and
 * `includeRelated`, the stats span the set's whole group (root + related sub-sets) — the
 * mirror of the catalog's include-related scope, so the value matches that browse view.
 * `bulkMaxCents` (the user's bulk-threshold preference, in cents) sets the cutoff the
 * server splits the bulk subtotal at; omitted = the server default ($1). */
export function getCollectionSummary(
  token: string,
  game: string,
  set?: string,
  includeRelated?: boolean,
  bulkMaxCents?: number,
): Promise<CollectionSummary> {
  // include_related only means anything alongside a set scope (matches the backend).
  const qs = listQuery({ set, includeRelated: set ? includeRelated : undefined, bulkMaxCents })
  return request<CollectionSummary>(`/api/collection/${encodeURIComponent(game)}/summary${qs}`, {
    token,
  })
}

/** The sets the user owns cards in, newest set first — the per-set collection landing.
 * `bulkMaxCents` sets each tile's bulk cutoff, matching the summary header (omitted = $1). */
export function getCollectionSets(
  token: string,
  game: string,
  bulkMaxCents?: number,
): Promise<{ data: CollectionSet[] }> {
  const qs = listQuery({ bulkMaxCents })
  return request<{ data: CollectionSet[] }>(
    `/api/collection/${encodeURIComponent(game)}/sets${qs}`,
    { token },
  )
}

/** A page of collection drop groups — `total`/pagination count *drops*, not cards. */
export type CollectionDropGroupPage = Page<CollectionDropGroup>

/** Params for the by-drop owned-cards view (paginated by drop). */
export interface CollectionDropsParams {
  page?: number
  pageSize?: number
  /** Scryfall-style search query (same syntax as the catalog card lists). */
  q?: string
}

/** Relative `/api/collection/{game}/sets/{code}/drops` path (paginated by drop). */
export function collectionSetDropsPath(
  game: string,
  code: string,
  params: CollectionDropsParams = {},
): string {
  const g = encodeURIComponent(game)
  const c = encodeURIComponent(code)
  return `/api/collection/${g}/sets/${c}/drops${listQuery(params)}`
}

/** The signed-in user's owned cards in a drop-grouped set (e.g. Secret Lair), grouped by
 * Secret Lair drop and paginated by drop. Only valid for sets where `has_drops` is true. */
export function getCollectionSetDrops(
  token: string,
  game: string,
  code: string,
  params?: CollectionDropsParams,
): Promise<CollectionDropGroupPage> {
  return request<CollectionDropGroupPage>(collectionSetDropsPath(game, code, params), { token })
}

/** A page of collection sub-type groups — `total`/pagination count *sub-types*, not cards. */
export type CollectionSubtypeGroupPage = Page<CollectionSubtypeGroup>

/** Relative `/api/collection/{game}/sets/{code}/subtypes` path (paginated by sub-type).
 * Same param shape as the by-drop path. */
export function collectionSetSubtypesPath(
  game: string,
  code: string,
  params: CollectionDropsParams = {},
): string {
  const g = encodeURIComponent(game)
  const c = encodeURIComponent(code)
  return `/api/collection/${g}/sets/${c}/subtypes${listQuery(params)}`
}

/** The signed-in user's owned cards in a set, grouped by card sub-type (treatment) and
 * paginated by sub-type. Offered where the tile's `has_subtypes` is true. */
export function getCollectionSetSubtypes(
  token: string,
  game: string,
  code: string,
  params?: CollectionDropsParams,
): Promise<CollectionSubtypeGroupPage> {
  return request<CollectionSubtypeGroupPage>(collectionSetSubtypesPath(game, code, params), {
    token,
  })
}

/**
 * Max ids per `.../owned` request. Kept safely under the server's 500-id cap so we can
 * split an arbitrarily large page (e.g. a drop-grouped set whose by-drop page flattens
 * to a big trailing "Other" group) into batches rather than tripping the cap — which
 * would 422 and silently drop every badge on the page.
 */
const OWNED_BATCH_SIZE = 400

/**
 * Owned counts for the given card ids that the user owns, keyed by external id (cards
 * they don't own are simply absent). Sent as a POST rather than a GET query so a big
 * browse page's id list can't blow the request-line length behind a proxy, and split
 * into batches under the server's id cap so any page size works; the batch maps are
 * merged (batches are disjoint slices, so there's nothing to reconcile).
 */
export async function getCollectionOwned(
  token: string,
  game: string,
  ids: string[],
): Promise<OwnedCountsMap> {
  if (ids.length === 0) return {}
  const path = `/api/collection/${encodeURIComponent(game)}/owned`
  const batches: string[][] = []
  for (let i = 0; i < ids.length; i += OWNED_BATCH_SIZE) {
    batches.push(ids.slice(i, i + OWNED_BATCH_SIZE))
  }
  const responses = await Promise.all(
    batches.map((batch) =>
      request<{ data: OwnedCountsMap }>(path, { method: 'POST', body: { ids: batch }, token }),
    ),
  )
  return Object.assign({}, ...responses.map((response) => response.data))
}

/** A single day of the collection's total-value series, shaped like the price chart's
 * `PricePointLike` so it feeds the shared `PriceChart` unchanged: `usd` is the day's total
 * collection value and there's no separate foil line (`usd_foil` is always null). */
export interface CollectionValueSeriesPoint {
  date: string
  usd: string | null
  usd_foil: string | null
}

/** Relative `/api/collection/{game}/value-history` path, with an optional `range`. */
export function collectionValueHistoryPath(game: string, range?: PriceRange): string {
  const qs = range ? `?range=${encodeURIComponent(range)}` : ''
  return `/api/collection/${encodeURIComponent(game)}/value-history${qs}`
}

/**
 * The signed-in user's total collection value over time for a game, across the same
 * `?range` windows as the per-card price chart. The wire DTO's `value_usd` is mapped onto
 * the chart's `usd` field (with `usd_foil` null — a single total line), so the shared
 * `PriceChart` renders it without changes.
 */
export async function getCollectionValueHistory(
  token: string,
  game: string,
  range?: PriceRange,
): Promise<{ data: CollectionValueSeriesPoint[] }> {
  const response = await request<{ data: CollectionValuePoint[] }>(
    collectionValueHistoryPath(game, range),
    { token },
  )
  return {
    data: response.data.map((point) => ({
      date: point.date,
      usd: point.value_usd,
      usd_foil: null,
    })),
  }
}

/** How many copies of one card the user owns (zeros when not in the collection). */
export function getCollectionEntry(
  token: string,
  game: string,
  id: string,
): Promise<CollectionQuantities> {
  return request<CollectionQuantities>(collectionEntryPath(game, id), { token })
}

/** Set the owned counts for one card (absolute, not a delta). Both zero removes it. */
export function setCollectionEntry(
  token: string,
  game: string,
  id: string,
  body: CollectionQuantities,
): Promise<CollectionQuantities> {
  return request<CollectionQuantities>(collectionEntryPath(game, id), {
    method: 'PUT',
    body,
    token,
  })
}

/** Which provider-shaped CSV a collection export produces. */
export type CollectionExportFormat = 'archidekt' | 'moxfield'

/** Relative `/api/collection/{game}/export` path for a CSV export in the given shape. */
export function collectionExportPath(game: string, format: CollectionExportFormat): string {
  return `/api/collection/${encodeURIComponent(game)}/export?format=${format}`
}

/**
 * Download the signed-in user's whole collection as a provider-shaped CSV (Archidekt or
 * Moxfield). The response is a file, not JSON, so it can't go through `request` (which
 * parses JSON and hides the raw body) — we `fetch` directly with the bearer token and read
 * the blob. Throwing `ApiError` on a non-2xx keeps the auth store's single 401-refresh
 * retry working (a bare `fetch` resolves on 401 and would skip the refresh).
 */
export async function exportCollectionCsv(
  token: string,
  game: string,
  format: CollectionExportFormat,
): Promise<Blob> {
  const response = await fetch(`${API_URL}${collectionExportPath(game, format)}`, {
    headers: { Authorization: `Bearer ${token}` },
    credentials: 'include',
  })
  if (!response.ok) {
    throw new ApiError(`Export failed with status ${response.status}`, response.status)
  }
  return response.blob()
}
