import type { Card } from './catalog'
import { request } from './client'

// ---------- Collections (per-user, authenticated) ----------
//
// Every call takes an access `token` (obtained via the auth store's `authFetch`,
// which the `useAuthed*` composables wire up). Card ids are the same external ids
// the public catalog exposes. Paths are built here so they can be unit-tested; the
// heavy lifting (auth header, credentials, JSON) lives in `request`.

/** How many copies of a card a user owns. */
export interface CollectionQuantities {
  quantity: number
  foil_quantity: number
}

/** One owned card: the full public card payload plus the owned counts. */
export interface CollectionEntry {
  card: Card
  quantity: number
  foil_quantity: number
}

/** A page of collection entries plus pagination cursors. */
export interface CollectionPage {
  data: CollectionEntry[]
  page: number
  page_size: number
  total: number
  has_more: boolean
}

/** Aggregate stats for a user's per-game collection. */
export interface CollectionSummary {
  /** Distinct cards owned. */
  unique_cards: number
  /** Total copies owned (regular + foil). */
  total_cards: number
  /** Estimated USD value as a decimal string, or null when nothing is priced. */
  total_value_usd: string | null
}

export interface CollectionListParams {
  page?: number
  pageSize?: number
}

/** Relative `/api/collection/...` path for a user's collection in a game. */
export function collectionPath(game: string, params: CollectionListParams = {}): string {
  const search = new URLSearchParams()
  if (params.page) search.set('page', String(params.page))
  if (params.pageSize) search.set('page_size', String(params.pageSize))
  const qs = search.toString()
  return `/api/collection/${encodeURIComponent(game)}${qs ? `?${qs}` : ''}`
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

/** Aggregate stats (unique cards, total copies, estimated value) for the collection. */
export function getCollectionSummary(token: string, game: string): Promise<CollectionSummary> {
  return request<CollectionSummary>(`/api/collection/${encodeURIComponent(game)}/summary`, {
    token,
  })
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

// ---------- Import / sync from an external collection provider ----------
//
// A signed-in user can import their collection from an external service (Archidekt
// today; Moxfield planned). The backend fetches server-side and reconciles into the
// local collection, so the client only sends the provider + a URL/id + a mode.

/** Collection providers we can import from. */
export type CollectionProvider = 'archidekt'

/**
 * How an import reconciles with the existing collection:
 * - `overwrite` — set matched cards to the imported counts; leave other cards alone.
 * - `replace` — mirror the import exactly (also removes owned cards not in the import).
 * - `merge` — add the imported counts on top of what's owned.
 */
export type ReconcileMode = 'overwrite' | 'replace' | 'merge'

/** A background import/sync job's status (imports run async, throttled by the provider
 * rate limit, so the client polls this until a terminal status). */
export interface ImportJob {
  job_id: number
  status: 'queued' | 'running' | 'complete' | 'error'
  /** Present only when `status === 'complete'`. */
  summary?: ImportSummary
  /** Present only when `status === 'error'`. */
  error?: string
}

/** The outcome of an import, for user feedback. */
export interface ImportSummary {
  provider: string
  mode: ReconcileMode
  total_rows: number
  distinct_cards: number
  matched_cards: number
  unmatched_cards: number
  unmatched_sample: string[]
  regular_copies: number
  foil_copies: number
  removed_cards: number
}

/** A saved external collection link for a game. */
export interface CollectionSource {
  provider: string
  external_id: string
  /** A canonical, user-facing URL for the collection on the provider. */
  url: string
  /** RFC3339 timestamp of the last successful sync, or null if never synced. */
  last_synced_at: string | null
}

export interface ImportCollectionBody {
  provider: CollectionProvider
  source: string
  mode: ReconcileMode
}

export interface SaveSourceBody {
  provider: CollectionProvider
  source: string
}

/** `/api/collection/{game}/import` path. */
export function collectionImportPath(game: string): string {
  return `/api/collection/${encodeURIComponent(game)}/import`
}

/** `/api/collection/{game}/source` path. */
export function collectionSourcePath(game: string): string {
  return `/api/collection/${encodeURIComponent(game)}/source`
}

/** `/api/collection/{game}/sync` path. */
export function collectionSyncPath(game: string): string {
  return `/api/collection/${encodeURIComponent(game)}/sync`
}

/** `/api/collection/{game}/import/jobs/{jobId}` path. */
export function collectionImportJobPath(game: string, jobId: number): string {
  return `/api/collection/${encodeURIComponent(game)}/import/jobs/${jobId}`
}

/** Enqueue a one-off import from a provider (chosen reconcile mode). Returns a job to
 * poll — the fetch + reconcile run in the background, throttled by the provider rate
 * limit. */
export function importCollection(
  token: string,
  game: string,
  body: ImportCollectionBody,
): Promise<ImportJob> {
  return request<ImportJob>(collectionImportPath(game), { method: 'POST', body, token })
}

/** Poll a background import/sync job's status. */
export function getImportJob(token: string, game: string, jobId: number): Promise<ImportJob> {
  return request<ImportJob>(collectionImportJobPath(game, jobId), { token })
}

/** The saved collection link for a game, or null when none is saved. */
export function getCollectionSource(token: string, game: string): Promise<CollectionSource | null> {
  // A `null` body comes back from `request` as `undefined`; normalise it so callers
  // (and vue-query, which forbids `undefined` query results) always see `null`.
  return request<CollectionSource | null>(collectionSourcePath(game), { token }).then(
    (source) => source ?? null,
  )
}

/** Save (upsert) the collection link for a game. Validates the source; does not sync. */
export function saveCollectionSource(
  token: string,
  game: string,
  body: SaveSourceBody,
): Promise<CollectionSource> {
  return request<CollectionSource>(collectionSourcePath(game), { method: 'PUT', body, token })
}

/** Forget the saved collection link for a game. */
export function deleteCollectionSource(token: string, game: string): Promise<void> {
  return request<void>(collectionSourcePath(game), { method: 'DELETE', token })
}

/** Enqueue a re-sync from the saved collection link (mirror/replace). Returns a job to
 * poll (runs in the background, throttled by the provider rate limit). */
export function syncCollectionSource(token: string, game: string): Promise<ImportJob> {
  return request<ImportJob>(collectionSyncPath(game), { method: 'POST', token })
}
