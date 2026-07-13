import type { Ref } from 'vue'
import { keepPreviousData, useQueryClient, type QueryClient } from '@tanstack/vue-query'
import {
  getWishlist,
  getWishlistCounts,
  getWishlistEntry,
  getWishlistProductEntry,
  getWishlistProducts,
  getWishlistSetDrops,
  getWishlistSets,
  getWishlistSetSubtypes,
  getWishlistSummary,
  setWishlistEntry,
  setWishlistProductEntry,
} from '@/lib/api'
import type { ApiError, CollectionQuantities, WishlistProductPage } from '@/lib/api'
import { makeHoldingQueries, type SetHoldingVars } from '@/composables/holdingQueries'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'

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

// ---------- Wanted sealed products (wishlist-only, issue #364) ----------
//
// Sealed products can be wished for, but have no collection twin, so the seam rule
// (extend the shared engine, never fork it) doesn't apply — these are wishlist-only
// siblings that live beside the factory instance rather than as a factory axis. The key
// families head-start with `wishlist` so `useAuthCacheReset` wipes them on identity
// change; element-wise partial matching keeps `['wishlist-products', …]` independent of
// the card factory's `['wishlist', game]` invalidation. As in `holdingQueries.ts`, each
// vue-query option object is an intermediate variable (not an inline literal) so
// TanStack's deeply-reactive types don't trip excess-property checks through the authed
// wrappers.

/** Page size for the wanted sealed-products list (mirrors the sealed browse grid). */
export const WISHLIST_PRODUCT_PAGE_SIZE = 60

/** A page of the user's wanted sealed products, newest edit first. */
export function useWishlistProductsQuery(game: Ref<string>, page: Ref<number>) {
  const options = {
    queryKey: ['wishlist-products', game, page],
    queryFn: (token: string) =>
      getWishlistProducts(token, game.value, {
        page: page.value,
        pageSize: WISHLIST_PRODUCT_PAGE_SIZE,
      }),
    placeholderData: keepPreviousData,
  }
  return useAuthedQuery<WishlistProductPage>(options)
}

/** How many of one sealed product the user wants — for the product-page/dialog steppers.
 * `enabled`/`staleTime` as in the card entry hook: pass `staleTime: 0` + an open-gate so
 * absolute-count editors never seed off a stale cached holding. */
export function useWishlistProductEntryQuery(
  game: Ref<string>,
  id: Ref<string>,
  opts: { enabled?: Ref<boolean>; staleTime?: number } = {},
) {
  const options = {
    queryKey: ['wishlist-product-entry', game, id],
    queryFn: (token: string) => getWishlistProductEntry(token, game.value, id.value),
    enabled: opts.enabled,
    staleTime: opts.staleTime,
  }
  return useAuthedQuery<CollectionQuantities>(options)
}

/** Refresh the views that depend on wanted sealed products after a product write. */
export function invalidateWishlistProducts(
  qc: QueryClient,
  game: string,
  opts: { entryId?: string } = {},
) {
  qc.invalidateQueries({ queryKey: ['wishlist-products', game] })
  qc.invalidateQueries({
    queryKey: opts.entryId
      ? ['wishlist-product-entry', game, opts.entryId]
      : ['wishlist-product-entry', game],
  })
}

/** Variables for a sealed-product want write: which product, and the absolute counts. */
export type SetWishlistProductVars = SetHoldingVars

/** Set the wanted counts for a sealed product, then invalidate the dependent views. */
export function useSetWishlistProductEntryMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: SetWishlistProductVars) =>
      setWishlistProductEntry(token, vars.game, vars.id, {
        quantity: vars.quantity,
        foil_quantity: vars.foil_quantity,
      }),
    onSuccess: (data: CollectionQuantities, vars: SetWishlistProductVars) => {
      qc.setQueryData(['wishlist-product-entry', vars.game, vars.id], data)
    },
    onSettled: (
      _data: CollectionQuantities | undefined,
      _error: ApiError | null,
      vars: SetWishlistProductVars,
    ) => {
      invalidateWishlistProducts(qc, vars.game, { entryId: vars.id })
    },
  }
  return useAuthedMutation<CollectionQuantities, SetWishlistProductVars>(options)
}
