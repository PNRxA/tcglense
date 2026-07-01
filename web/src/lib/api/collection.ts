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
