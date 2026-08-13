import { request } from './client'
import type { CollectionProvider, ImportJob, ImportSummary, ReconcileMode } from './generated'

// ---------- Import from an external collection provider ----------
//
// A signed-in user can import their collection from an external service (Archidekt or
// Moxfield). The backend fetches server-side and reconciles into the local collection,
// so the client only sends the provider + a URL/id + a mode. Every import is one-off —
// nothing is remembered between them. The wire types are generated from the API's Rust
// DTOs into `./generated` and re-exported here.
//
// Not every service has a public API to fetch from. Mythic Tools (issue #572) is a phone
// app, so its collections arrive as an export the user uploads or — far easier from a
// phone — pastes: `importCollectionText`. Both that and `importCollectionCsv` post raw
// content the server sniffs, so neither asks the user to name their format.

export type {
  CollectionProvider,
  ImportJob,
  ImportProgress,
  ImportSummary,
  ReconcileMode,
} from './generated'

/** Human-readable provider names, for labels and copy. */
export const PROVIDER_LABELS: Record<CollectionProvider, string> = {
  archidekt: 'Archidekt',
  moxfield: 'Moxfield',
  mythictools: 'Mythic Tools',
}

// The request body stays hand-written: the wire `ImportRequest` accepts any `provider`
// string (validated server-side), while the client deliberately narrows it to the known
// `CollectionProvider` union.

export interface ImportCollectionBody {
  provider: CollectionProvider
  source: string
  mode: ReconcileMode
}

/** `/api/collection/{game}/import` path. */
export function collectionImportPath(game: string): string {
  return `/api/collection/${encodeURIComponent(game)}/import`
}

/** `/api/collection/{game}/import/jobs/{jobId}` path. */
export function collectionImportJobPath(game: string, jobId: number): string {
  return `/api/collection/${encodeURIComponent(game)}/import/jobs/${jobId}`
}

/**
 * Largest upload the server accepts, for both the file and paste imports (kept in sync
 * with the API's `MAX_CSV_UPLOAD_BYTES`). Used for a friendly client-side pre-check so an
 * oversized file is rejected with a clear message rather than a bare `413`.
 */
export const MAX_CSV_UPLOAD_BYTES = 16 * 1024 * 1024

/** `/api/collection/{game}/import/csv?mode=...` path. */
export function collectionImportCsvPath(game: string, mode: ReconcileMode): string {
  const search = new URLSearchParams({ mode })
  return `/api/collection/${encodeURIComponent(game)}/import/csv?${search.toString()}`
}

/** `/api/collection/{game}/import/text?mode=...` path. */
export function collectionImportTextPath(game: string, mode: ReconcileMode): string {
  const search = new URLSearchParams({ mode })
  return `/api/collection/${encodeURIComponent(game)}/import/text?${search.toString()}`
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

/** Poll a background import job's status. */
export function getImportJob(token: string, game: string, jobId: number): Promise<ImportJob> {
  return request<ImportJob>(collectionImportJobPath(game, jobId), { token })
}

/**
 * Import a collection from an uploaded export file — an Archidekt, Moxfield or Mythic
 * Tools CSV, or a plain-text card list; the server detects which from the content. The
 * file is sent as the raw request body and reconciled server-side; unlike the URL import
 * it needs no upstream fetch, so it resolves **synchronously** to the
 * {@link ImportSummary} (no job to poll).
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

/**
 * Import a collection from text the user pasted: a card list (`2 Sol Ring (C21) 263`, one
 * per line) or the contents of a CSV export. Same sniffing, same synchronous
 * {@link ImportSummary} as {@link importCollectionCsv} — this exists because copying an
 * export out of a phone app (Mythic Tools, issue #572) is much easier than saving it to a
 * file and finding it in a browser's file picker.
 */
export function importCollectionText(
  token: string,
  game: string,
  text: string,
  mode: ReconcileMode,
): Promise<ImportSummary> {
  return request<ImportSummary>(collectionImportTextPath(game, mode), {
    method: 'POST',
    token,
    rawBody: text,
    contentType: 'text/plain',
  })
}
