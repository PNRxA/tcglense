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

// ---------- Wish list (per-user, authenticated) ----------
//
// The collection's twin for cards the user *wants to buy* (issue #167): the same
// holding shape and endpoints (minus import/sync — a wish list has nothing to sync
// from), under `/api/wishlist/...`. Every call takes an access `token` (obtained via
// the auth store's `authFetch`, which the `useAuthed*` composables wire up). Card ids
// are the same external ids the public catalog exposes. Paths are built here so they
// can be unit-tested. The wire shapes are the collection's exact ones (the backend
// reuses those DTOs), so the `Collection*` types from `./collection` are reused here
// rather than duplicated.

/** Relative `/api/wishlist/...` path for a user's wish list in a game. */
export function wishlistPath(game: string, params: CollectionListParams = {}): string {
  return `/api/wishlist/${encodeURIComponent(game)}${listQuery(params)}`
}

/** Relative `/api/wishlist/{game}/cards/{id}` path for one card's wanted counts. */
export function wishlistEntryPath(game: string, id: string): string {
  return `/api/wishlist/${encodeURIComponent(game)}/cards/${encodeURIComponent(id)}`
}

/** The signed-in user's wishlisted cards for a game, most-recently-updated first. */
export function getWishlist(
  token: string,
  game: string,
  params?: CollectionListParams,
): Promise<CollectionPage> {
  return request<CollectionPage>(wishlistPath(game, params), { token })
}

/** Aggregate stats (unique cards, total copies, estimated value) for the wish list,
 * optionally scoped to a single set (the per-set wish-list view). With a `set` and
 * `includeRelated`, the stats span the set's whole group (root + related sub-sets) — the
 * mirror of the catalog's include-related scope, so the value matches that browse view. */
export function getWishlistSummary(
  token: string,
  game: string,
  set?: string,
  includeRelated?: boolean,
): Promise<CollectionSummary> {
  // include_related only means anything alongside a set scope (matches the backend).
  const qs = listQuery({ set, includeRelated: set ? includeRelated : undefined })
  return request<CollectionSummary>(`/api/wishlist/${encodeURIComponent(game)}/summary${qs}`, {
    token,
  })
}

/** The sets the user has wishlisted cards in, newest set first — the per-set counts
 * and values overlaid on the wish-list landing's all-sets grid. */
export function getWishlistSets(token: string, game: string): Promise<{ data: CollectionSet[] }> {
  return request<{ data: CollectionSet[] }>(`/api/wishlist/${encodeURIComponent(game)}/sets`, {
    token,
  })
}

/** Relative `/api/wishlist/{game}/sets/{code}/drops` path (paginated by drop). */
export function wishlistSetDropsPath(
  game: string,
  code: string,
  params: CollectionDropsParams = {},
): string {
  const g = encodeURIComponent(game)
  const c = encodeURIComponent(code)
  return `/api/wishlist/${g}/sets/${c}/drops${listQuery(params)}`
}

/** The signed-in user's wishlisted cards in a drop-grouped set (e.g. Secret Lair),
 * grouped by Secret Lair drop and paginated by drop. Only valid where `has_drops` is
 * true. */
export function getWishlistSetDrops(
  token: string,
  game: string,
  code: string,
  params?: CollectionDropsParams,
): Promise<CollectionDropGroupPage> {
  return request<CollectionDropGroupPage>(wishlistSetDropsPath(game, code, params), { token })
}

/** Relative `/api/wishlist/{game}/sets/{code}/subtypes` path (paginated by sub-type). */
export function wishlistSetSubtypesPath(
  game: string,
  code: string,
  params: CollectionDropsParams = {},
): string {
  const g = encodeURIComponent(game)
  const c = encodeURIComponent(code)
  return `/api/wishlist/${g}/sets/${c}/subtypes${listQuery(params)}`
}

/** The signed-in user's wanted cards in a set, grouped by card sub-type (treatment) and
 * paginated by sub-type — the wish-list mirror of `getCollectionSetSubtypes`. */
export function getWishlistSetSubtypes(
  token: string,
  game: string,
  code: string,
  params?: CollectionDropsParams,
): Promise<CollectionSubtypeGroupPage> {
  return request<CollectionSubtypeGroupPage>(wishlistSetSubtypesPath(game, code, params), { token })
}

/**
 * Max ids per `.../counts` request. Kept safely under the server's 500-id cap so we can
 * split an arbitrarily large page (e.g. a drop-grouped set whose by-drop page flattens
 * to a big trailing "Other" group) into batches rather than tripping the cap — which
 * would 422 and silently drop every badge on the page.
 */
const COUNTS_BATCH_SIZE = 400

/**
 * Wanted counts for the given card ids that are on the user's wish list, keyed by
 * external id (cards not on the list are simply absent). The route is `/counts` — not
 * the collection's `/owned`, since a wish list doesn't track ownership — but otherwise
 * behaves identically: a POST rather than a GET query so a big browse page's id list
 * can't blow the request-line length behind a proxy, split into batches under the
 * server's id cap so any page size works; the batch maps are merged (batches are
 * disjoint slices, so there's nothing to reconcile).
 */
export async function getWishlistCounts(
  token: string,
  game: string,
  ids: string[],
): Promise<OwnedCountsMap> {
  if (ids.length === 0) return {}
  const path = `/api/wishlist/${encodeURIComponent(game)}/counts`
  const batches: string[][] = []
  for (let i = 0; i < ids.length; i += COUNTS_BATCH_SIZE) {
    batches.push(ids.slice(i, i + COUNTS_BATCH_SIZE))
  }
  const responses = await Promise.all(
    batches.map((batch) =>
      request<{ data: OwnedCountsMap }>(path, { method: 'POST', body: { ids: batch }, token }),
    ),
  )
  return Object.assign({}, ...responses.map((response) => response.data))
}

/** How many copies of one card the user wants (zeros when not on the wish list). */
export function getWishlistEntry(
  token: string,
  game: string,
  id: string,
): Promise<CollectionQuantities> {
  return request<CollectionQuantities>(wishlistEntryPath(game, id), { token })
}

/** Set the wanted counts for one card (absolute, not a delta). Both zero removes it. */
export function setWishlistEntry(
  token: string,
  game: string,
  id: string,
  body: CollectionQuantities,
): Promise<CollectionQuantities> {
  return request<CollectionQuantities>(wishlistEntryPath(game, id), {
    method: 'PUT',
    body,
    token,
  })
}
