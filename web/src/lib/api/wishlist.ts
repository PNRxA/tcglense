import { makeHoldingApi } from './holdings'
import { makeProductHoldingApi } from './product-holdings'
import type { CollectionSet, CollectionSummary } from './generated'

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

// Sealed products instantiate the same product-holding client as the collection (#435).
const productApi = makeProductHoldingApi('wishlist', 'counts')

export type {
  ProductHoldingEntry,
  ProductHoldingSetGroup,
  ProductHoldingSummary,
} from './generated'
export type {
  ProductHoldingEntry as WishlistProductEntry,
  ProductHoldingSetGroup as WishlistProductSetGroup,
  ProductHoldingSummary as WishlistProductSummary,
} from './generated'
export type {
  ProductHoldingPage as WishlistProductPage,
  ProductHoldingSetPage as WishlistProductSetPage,
} from './product-holdings'

export const wishlistProductsPath = productApi.productsPath
export const wishlistProductEntryPath = productApi.entryPath
export const wishlistProductSummaryPath = productApi.summaryPath
export const wishlistProductCountsPath = productApi.countsPath
export const getWishlistProducts = productApi.list
/** The signed-in user's wanted sealed products grouped by set (newest set first, products
 * name-sorted within each), paginated by set — the by-set wish-list landing view. */
export const getWishlistProductsBySet = productApi.listBySet
export const getWishlistProductEntry = productApi.getEntry
export const setWishlistProductEntry = productApi.setEntry
export const getWishlistProductSummary = productApi.summary
export const getWishlistProductCounts = productApi.counts
