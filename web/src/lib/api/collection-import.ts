import { request } from './client'
import type {
  CollectionProvider,
  CollectionSource,
  ImportJob,
  ImportSummary,
  ReconcileMode,
} from './generated'

// ---------- Import / sync from an external collection provider ----------
//
// A signed-in user can import their collection from an external service (Archidekt
// today; Moxfield planned). The backend fetches server-side and reconciles into the
// local collection, so the client only sends the provider + a URL/id + a mode. The
// wire types are generated from the API's Rust DTOs into `./generated` and
// re-exported here.

export type {
  CollectionProvider,
  CollectionSource,
  ImportJob,
  ImportSummary,
  ReconcileMode,
} from './generated'

// The request bodies stay hand-written: the wire `ImportRequest`/`SaveSourceRequest`
// accept any `provider` string (validated server-side), while the client deliberately
// narrows it to the known `CollectionProvider` union.

export interface ImportCollectionBody {
  provider: CollectionProvider
  source: string
  mode: ReconcileMode
}

export interface SaveSourceBody {
  provider: CollectionProvider
  source: string
  /** Whether saved re-syncs should use smart (incremental) sync. Defaults false server-side. */
  smart?: boolean
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

/**
 * Largest CSV upload the server accepts (kept in sync with the API's
 * `MAX_CSV_UPLOAD_BYTES`). Used for a friendly client-side pre-check so an oversized
 * file is rejected with a clear message rather than a bare `413`.
 */
export const MAX_CSV_UPLOAD_BYTES = 16 * 1024 * 1024

/** `/api/collection/{game}/import/csv?mode=...` path. */
export function collectionImportCsvPath(game: string, mode: ReconcileMode): string {
  const search = new URLSearchParams({ mode })
  return `/api/collection/${encodeURIComponent(game)}/import/csv?${search.toString()}`
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

/**
 * Import a collection from an uploaded Archidekt CSV export. The file is sent as the raw
 * request body (there's no persistent source to re-sync, so this is always one-off) and
 * reconciled server-side; unlike the URL import it needs no upstream fetch, so it
 * resolves **synchronously** to the {@link ImportSummary} (no job to poll).
 */
export function importCollectionCsv(
  token: string,
  game: string,
  file: File | Blob,
  mode: ReconcileMode,
): Promise<ImportSummary> {
  return request<ImportSummary>(collectionImportCsvPath(game, mode), {
    method: 'POST',
    token,
    rawBody: file,
    contentType: 'text/csv',
  })
}
