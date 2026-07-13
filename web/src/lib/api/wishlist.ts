import { makeHoldingApi, postCountsBatched } from './holdings'
import { listQuery, request } from './client'
import type { OwnedCountsMap } from './collection'
import type {
  CollectionQuantities,
  CollectionSet,
  CollectionSummary,
  Page,
  WishlistProductEntry,
  WishlistProductSummary,
} from './generated'

// ---------- Wish list (per-user, authenticated) ----------
//
// The collection's twin for cards the user *wants to buy* (issue #167): the same
// holding shape and endpoints (minus import/sync — a wish list has nothing to sync
// from), under `/api/wishlist/...`. The wire shapes are the collection's exact ones
// (the backend reuses those DTOs), so the shared holding core is instantiated from
// `makeHoldingApi` — the wish list is the `'wishlist'` instance whose batch-counts
// leaf is `/counts` (not the collection's `/owned`, since a wish list doesn't track
// ownership). The re-exported names below keep their exact signatures. Every call takes
// an access `token` (obtained via the auth store's `authFetch`, which the `useAuthed*`
// composables wire up). Card ids are the same external ids the public catalog exposes.

const api = makeHoldingApi('wishlist', 'counts')

/** Relative `/api/wishlist/...` path for a user's wish list in a game. */
export const wishlistPath = api.path

/** Relative `/api/wishlist/{game}/cards/{id}` path for one card's wanted counts. */
export const wishlistEntryPath = api.entryPath

/** The signed-in user's wishlisted cards for a game, most-recently-updated first. */
export const getWishlist = api.list

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
  // A wish list has no bulk-threshold preference, so the summary takes no `bulkMaxCents`.
  return api.summary(token, game, set, includeRelated)
}

/** The sets the user has wishlisted cards in, newest set first — the per-set counts
 * and values overlaid on the wish-list landing's all-sets grid. */
export function getWishlistSets(token: string, game: string): Promise<{ data: CollectionSet[] }> {
  return api.sets(token, game)
}

/** Relative `/api/wishlist/{game}/sets/{code}/drops` path (paginated by drop). */
export const wishlistSetDropsPath = api.setDropsPath

/** The signed-in user's wishlisted cards in a drop-grouped set (e.g. Secret Lair),
 * grouped by Secret Lair drop and paginated by drop. Only valid where `has_drops` is
 * true. */
export const getWishlistSetDrops = api.getSetDrops

/** Relative `/api/wishlist/{game}/sets/{code}/subtypes` path (paginated by sub-type). */
export const wishlistSetSubtypesPath = api.setSubtypesPath

/** The signed-in user's wanted cards in a set, grouped by card sub-type (treatment) and
 * paginated by sub-type — the wish-list mirror of `getCollectionSetSubtypes`. */
export const getWishlistSetSubtypes = api.getSetSubtypes

/** Wanted counts for the given card ids that are on the user's wish list, keyed by
 * external id (cards not on the list are simply absent) — POSTed to `.../counts` and
 * batched under the id cap. */
export const getWishlistCounts = api.counts

/** How many copies of one card the user wants (zeros when not on the wish list). */
export const getWishlistEntry = api.getEntry

/** Set the wanted counts for one card (absolute, not a delta). Both zero removes it. */
export const setWishlistEntry = api.setEntry

// ---------- Wanted sealed products (wishlist-only, issue #364) ----------
//
// Sealed products can be wished for too, but — unlike cards — there is no collection
// twin (the collection deliberately has no sealed surface), so these live beside the
// `makeHoldingApi` instance rather than as a factory axis. The endpoints mirror the card
// wish list's per-entry read/write, but the list carries the full public product payload
// (`WishlistProductEntry = { product, quantity, foil_quantity }`) and is fixed
// recency-desc (no q/sort). The id on the wire is the external (TCGplayer) product id.

export type { WishlistProductEntry, WishlistProductSummary } from './generated'

/** A page of the user's wanted sealed products plus pagination cursors. */
export type WishlistProductPage = Page<WishlistProductEntry>

/** Relative `/api/wishlist/{game}/products` path (paged). */
export function wishlistProductsPath(
  game: string,
  params: { page?: number; pageSize?: number } = {},
): string {
  return `/api/wishlist/${encodeURIComponent(game)}/products${listQuery(params)}`
}

/** Relative `/api/wishlist/{game}/products/{id}` path for one product's wanted counts. */
export function wishlistProductEntryPath(game: string, id: string): string {
  return `/api/wishlist/${encodeURIComponent(game)}/products/${encodeURIComponent(id)}`
}

/** The signed-in user's wanted sealed products, most-recently-updated first. */
export function getWishlistProducts(
  token: string,
  game: string,
  params?: { page?: number; pageSize?: number },
): Promise<WishlistProductPage> {
  return request<WishlistProductPage>(wishlistProductsPath(game, params), { token })
}

/** How many of one sealed product the user wants (zeros when not on the wish list). */
export function getWishlistProductEntry(
  token: string,
  game: string,
  id: string,
): Promise<CollectionQuantities> {
  return request<CollectionQuantities>(wishlistProductEntryPath(game, id), { token })
}

/** Set the wanted counts for one sealed product (absolute, not a delta). Both zero removes it. */
export function setWishlistProductEntry(
  token: string,
  game: string,
  id: string,
  body: CollectionQuantities,
): Promise<CollectionQuantities> {
  return request<CollectionQuantities>(wishlistProductEntryPath(game, id), {
    method: 'PUT',
    body,
    token,
  })
}

/** Relative `/api/wishlist/{game}/products/summary` path (the sealed stats trio). */
export function wishlistProductSummaryPath(game: string): string {
  return `/api/wishlist/${encodeURIComponent(game)}/products/summary`
}

/** Aggregate stats (unique products, total copies, estimated cost) for the user's
 * wanted sealed products. `total_value_usd` is null when nothing wanted is priced. */
export function getWishlistProductSummary(
  token: string,
  game: string,
): Promise<WishlistProductSummary> {
  return request<WishlistProductSummary>(wishlistProductSummaryPath(game), { token })
}

/** Relative `/api/wishlist/{game}/products/counts` path (batch wanted counts). */
export function wishlistProductCountsPath(game: string): string {
  return `/api/wishlist/${encodeURIComponent(game)}/products/counts`
}

/** Wanted counts for the given external product ids that are on the user's wish list,
 * keyed by external id (products not on the list are simply absent) — POSTed and
 * batched under the id cap, exactly like the card counts. */
export function getWishlistProductCounts(
  token: string,
  game: string,
  ids: string[],
): Promise<OwnedCountsMap> {
  return postCountsBatched(wishlistProductCountsPath(game), token, ids)
}
