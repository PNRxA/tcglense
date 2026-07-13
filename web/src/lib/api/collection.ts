import { API_URL, ApiError, request } from './client'
import { makeHoldingApi } from './holdings'
import type { PriceRange } from './catalog'
import type {
  CollectionDropGroup,
  CollectionEntry,
  CollectionMovers,
  CollectionQuantities,
  CollectionSubtypeGroup,
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
//
// The holding core (path builders, list/summary/sets/drops/subtypes fetchers, the
// batched counts fetcher, and the single-entry get/set) is shared with the wish list
// via `makeHoldingApi` — the collection is just the `'collection'` instance whose
// batch-counts leaf is `/owned`. The re-exported names below keep their exact
// signatures. Collection-only surfaces (value history, CSV export) stay local.

export type {
  CollectionDropGroup,
  CollectionEntry,
  CollectionMover,
  CollectionMoverList,
  CollectionMovers,
  CollectionQuantities,
  CollectionSet,
  CollectionSubtypeGroup,
  CollectionSummary,
  CollectionVisibility,
} from './generated'

import type { CollectionVisibility } from './generated'

/** Whether the signed-in user's collection for a game is public, plus their handle. */
export function getCollectionVisibility(
  token: string,
  game: string,
): Promise<CollectionVisibility> {
  return request<CollectionVisibility>(`/api/collection/${encodeURIComponent(game)}/visibility`, {
    token,
  })
}

/** Enable/disable public sharing for the signed-in user's collection in a game (issues
 * #361/#362). Enabling requires a username first — the server 409s otherwise, which the
 * SPA branches on to prompt the username step. */
export function setCollectionVisibility(
  token: string,
  game: string,
  isPublic: boolean,
): Promise<CollectionVisibility> {
  return request<CollectionVisibility>(`/api/collection/${encodeURIComponent(game)}/visibility`, {
    method: 'PUT',
    body: { public: isPublic },
    token,
  })
}

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

/** A page of collection drop groups — `total`/pagination count *drops*, not cards. */
export type CollectionDropGroupPage = Page<CollectionDropGroup>

/** Params for the by-drop owned-cards view (paginated by drop). */
export interface CollectionDropsParams {
  page?: number
  pageSize?: number
  /** Scryfall-style search query (same syntax as the catalog card lists). */
  q?: string
}

/** A page of collection sub-type groups — `total`/pagination count *sub-types*, not cards. */
export type CollectionSubtypeGroupPage = Page<CollectionSubtypeGroup>

// The shared holding core for a collection: base `/api/collection`, batch-counts leaf
// `/owned`. Each member is re-exported below under its existing name/signature.
const api = makeHoldingApi('collection', 'owned')

/** Relative `/api/collection/...` path for a user's collection in a game. */
export const collectionPath = api.path

/** Relative `/api/collection/{game}/cards/{id}` path for one card's holding. */
export const collectionEntryPath = api.entryPath

/** The signed-in user's owned cards for a game, most-recently-updated first. */
export const getCollection = api.list

/** Aggregate stats (unique cards, total copies, estimated value) for the collection,
 * optionally scoped to a single set (the per-set collection view). `bulkMaxCents` (in
 * cents) sets the cutoff the server splits the bulk subtotal at; omitted = the server
 * default ($1). */
export const getCollectionSummary = api.summary

/** The sets the user owns cards in, newest set first — the per-set collection landing.
 * `bulkMaxCents` sets each tile's bulk cutoff, matching the summary header. */
export const getCollectionSets = api.sets

/** Relative `/api/collection/{game}/sets/{code}/drops` path (paginated by drop). */
export const collectionSetDropsPath = api.setDropsPath

/** The signed-in user's owned cards in a drop-grouped set (e.g. Secret Lair), grouped by
 * Secret Lair drop and paginated by drop. Only valid for sets where `has_drops` is true. */
export const getCollectionSetDrops = api.getSetDrops

/** Relative `/api/collection/{game}/sets/{code}/subtypes` path (paginated by sub-type). */
export const collectionSetSubtypesPath = api.setSubtypesPath

/** The signed-in user's owned cards in a set, grouped by card sub-type (treatment) and
 * paginated by sub-type. Offered where the tile's `has_subtypes` is true. */
export const getCollectionSetSubtypes = api.getSetSubtypes

/** Owned counts for the given card ids that the user owns, keyed by external id (cards
 * they don't own are simply absent) — POSTed to `.../owned` and batched under the id cap. */
export const getCollectionOwned = api.counts

/** How many copies of one card the user owns (zeros when not in the collection). */
export const getCollectionEntry = api.getEntry

/** Set the owned counts for one card (absolute, not a delta). Both zero removes it. */
export const setCollectionEntry = api.setEntry

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

/** Relative `/api/collection/{game}/movers` path. */
export function collectionMoversPath(game: string): string {
  return `/api/collection/${encodeURIComponent(game)}/movers`
}

/**
 * The signed-in user's biggest gain/loss movements (day / week / month) across the cards
 * they own, ranked by the change in each holding's USD value. Per-user + authenticated.
 */
export async function getCollectionMovers(token: string, game: string): Promise<CollectionMovers> {
  return request<CollectionMovers>(collectionMoversPath(game), { token })
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
