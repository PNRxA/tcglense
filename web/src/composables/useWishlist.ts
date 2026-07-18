import {
  getWishlist,
  getWishlistCounts,
  getWishlistEntry,
  getWishlistProductCounts,
  getWishlistProductEntry,
  getWishlistProducts,
  getWishlistProductsBySet,
  getWishlistProductSummary,
  getWishlistSetDrops,
  getWishlistSets,
  getWishlistSetSubtypes,
  getWishlistSummary,
  setWishlistEntry,
  setWishlistProductEntry,
} from '@/lib/api'
import { makeHoldingQueries, type SetHoldingVars } from '@/composables/holdingQueries'
import {
  makeProductHoldingQueries,
  PRODUCT_HOLDING_PAGE_SIZE,
} from '@/composables/productHoldingQueries'

// Server state for the signed-in user's wish list (issue #167) — the collection's twin
// for cards they *want to buy*, minting a parallel `['wishlist', …]` query-key family
// over the same wire shapes. All the read hooks, the browse-badge counts hook, the
// invalidation helper, and the set-entry mutation are the shared holding composables (see
// `holdingQueries.ts`); this module instantiates that factory with the wish-list api
// functions and re-exports each member under its existing name/signature. A wish list has
// no bulk-threshold preference and no value history, so it opts out of both asymmetries.
const queries = makeHoldingQueries({
  prefix: 'wishlist',
  countsKey: 'wishlist-counts',
  getList: getWishlist,
  getSetDrops: getWishlistSetDrops,
  getSetSubtypes: getWishlistSetSubtypes,
  getSummary: getWishlistSummary,
  getSets: getWishlistSets,
  getEntry: getWishlistEntry,
  getCounts: getWishlistCounts,
  setEntry: setWishlistEntry,
  withBulkThreshold: false,
  invalidateValueHistory: false,
  deferListRefetch: true,
})

/**
 * Refresh every view that depends on the wish list's contents after a wish-list write
 * (a per-card edit — there's no import/sync path here). Covers the grid, the summary
 * header, the per-card wanted-count steppers, the by-drop view, the landing's per-set
 * counts, and the browse-grid wanted-count badges. Pass `entryId` to scope the per-card
 * entry invalidation to the edited card.
 */
export const invalidateWishlistData = queries.invalidate

/** A page of the user's wishlisted cards for a game. See `holdingQueries.useListQuery`. */
export const useWishlistQuery = queries.useListQuery

/** A page (by drop) of the user's wishlisted cards in a drop-grouped set (e.g. Secret Lair). */
export const useWishlistDropsQuery = queries.useDropsQuery

/** A page (by sub-type) of the user's wanted cards in a set, grouped by card treatment. */
export const useWishlistSubtypesQuery = queries.useSubtypesQuery

/** Aggregate stats for the wish list, optionally scoped to one set. */
export const useWishlistSummaryQuery = queries.useSummaryQuery

/** The sets the user has wishlisted cards in (newest first) — the per-set wish-list landing. */
export const useWishlistSetsQuery = queries.useSetsQuery

/** How many copies of one card the signed-in user wants — for the card-detail controls. */
export const useWishlistEntryQuery = queries.useEntryQuery

/** Wanted counts for the cards currently being browsed — the wish-list browse badges. */
export const useWishlistCounts = queries.useCounts

/** Variables for a wish-list write: which card, and the desired absolute counts. */
export type SetWishlistVars = SetHoldingVars

/** Set the wanted counts for a card, then invalidate the dependent wish-list views. */
export const useSetWishlistEntryMutation = queries.useSetEntryMutation

const productQueries = makeProductHoldingQueries({
  prefix: 'wishlist',
  getList: getWishlistProducts,
  getListBySet: getWishlistProductsBySet,
  getEntry: getWishlistProductEntry,
  getSummary: getWishlistProductSummary,
  getCounts: getWishlistProductCounts,
  setEntry: setWishlistProductEntry,
})

export const WISHLIST_PRODUCT_PAGE_SIZE = PRODUCT_HOLDING_PAGE_SIZE
export const useWishlistProductsQuery = productQueries.useProductsQuery
export const useWishlistProductsBySetQuery = productQueries.useProductsBySetQuery
export const useWishlistProductEntryQuery = productQueries.useEntryQuery
export const useWishlistProductSummaryQuery = productQueries.useSummaryQuery
export const useWishlistProductCounts = productQueries.useCounts
export const invalidateWishlistProducts = productQueries.invalidate
export type SetWishlistProductVars = SetHoldingVars
export const useSetWishlistProductEntryMutation = productQueries.useSetEntryMutation
