import type { Card, CardSet, Page } from './catalog'
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

/** Owned counts for a batch of cards, keyed by external card id (owned cards only). */
export type OwnedCountsMap = Record<string, CollectionQuantities>

/** A page of collection entries plus pagination cursors. */
export type CollectionPage = Page<CollectionEntry>

/** Aggregate stats for a user's per-game collection. */
export interface CollectionSummary {
  /** Distinct cards owned. */
  unique_cards: number
  /** Total copies owned (regular + foil). */
  total_cards: number
  /** Estimated USD value as a decimal string, or null when nothing is priced. */
  total_value_usd: string | null
}

/**
 * One set a user owns cards in, for the collection's per-set landing. Carries the same
 * catalog set metadata a `SetTile` needs (so the tile can be reused) plus how much of
 * the set the user owns.
 */
export interface CollectionSet extends CardSet {
  /** Distinct cards owned in this set. */
  owned_cards: number
  /** Total copies owned (regular + foil) in this set. */
  owned_copies: number
  /** Estimated USD value of the owned cards in this set as a decimal string, or null
   * when nothing owned in the set is priced. Same semantics as the summary value. */
  owned_value_usd: string | null
}

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
  const search = new URLSearchParams()
  if (params.page) search.set('page', String(params.page))
  if (params.pageSize) search.set('page_size', String(params.pageSize))
  if (params.q) search.set('q', params.q)
  if (params.sort) search.set('sort', params.sort)
  if (params.dir) search.set('dir', params.dir)
  if (params.set) search.set('set', params.set)
  if (params.includeRelated) search.set('include_related', 'true')
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

/** Aggregate stats (unique cards, total copies, estimated value) for the collection,
 * optionally scoped to a single set (the per-set collection view). With a `set` and
 * `includeRelated`, the stats span the set's whole group (root + related sub-sets) — the
 * mirror of the catalog's include-related scope, so the value matches that browse view. */
export function getCollectionSummary(
  token: string,
  game: string,
  set?: string,
  includeRelated?: boolean,
): Promise<CollectionSummary> {
  const search = new URLSearchParams()
  if (set) search.set('set', set)
  // include_related only means anything alongside a set scope (matches the backend).
  if (set && includeRelated) search.set('include_related', 'true')
  const qs = search.toString()
  return request<CollectionSummary>(
    `/api/collection/${encodeURIComponent(game)}/summary${qs ? `?${qs}` : ''}`,
    { token },
  )
}

/** The sets the user owns cards in, newest set first — the per-set collection landing. */
export function getCollectionSets(token: string, game: string): Promise<{ data: CollectionSet[] }> {
  return request<{ data: CollectionSet[] }>(`/api/collection/${encodeURIComponent(game)}/sets`, {
    token,
  })
}

/** One Secret Lair drop with the user's owned cards in it — the collection mirror of the
 * catalog `DropGroup`, but each card carries the owned counts. */
export interface CollectionDropGroup {
  /** Stable slug for anchors; null for the catch-all "Other" group. */
  slug: string | null
  title: string
  card_count: number
  cards: CollectionEntry[]
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
  const search = new URLSearchParams()
  if (params.page) search.set('page', String(params.page))
  if (params.pageSize) search.set('page_size', String(params.pageSize))
  if (params.q) search.set('q', params.q)
  const g = encodeURIComponent(game)
  const c = encodeURIComponent(code)
  const qs = search.toString()
  return `/api/collection/${g}/sets/${c}/drops${qs ? `?${qs}` : ''}`
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
