import type { Ref } from 'vue'
import { keepPreviousData, useQueryClient, type QueryClient } from '@tanstack/vue-query'
import type {
  ApiError,
  CollectionQuantities,
  OwnedCountsMap,
  ProductHoldingPage,
  ProductHoldingSetPage,
  ProductHoldingSummary,
} from '@/lib/api'
import { useBatchCounts, type SetHoldingVars } from '@/composables/holdingQueries'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'

export const PRODUCT_HOLDING_PAGE_SIZE = 60
/** Sets per page in the by-set holdings view — the page unit there is a set group, not a
 * product, so it's far smaller than the flat product page size above. */
export const PRODUCT_HOLDING_SET_PAGE_SIZE = 10

interface ProductHoldingQueriesConfig {
  prefix: 'collection' | 'wishlist'
  /** Collection product writes also change its value-history and movers analytics. */
  invalidateAnalytics?: boolean
  getList: (
    token: string,
    game: string,
    params?: { page?: number; pageSize?: number },
  ) => Promise<ProductHoldingPage>
  getListBySet: (
    token: string,
    game: string,
    params?: { page?: number; pageSize?: number },
  ) => Promise<ProductHoldingSetPage>
  getEntry: (token: string, game: string, id: string) => Promise<CollectionQuantities>
  getSummary: (token: string, game: string) => Promise<ProductHoldingSummary>
  getCounts: (token: string, game: string, ids: string[]) => Promise<OwnedCountsMap>
  setEntry: (
    token: string,
    game: string,
    id: string,
    body: CollectionQuantities,
  ) => Promise<CollectionQuantities>
}

/** Shared vue-query engine for collection and wish-list sealed-product holdings. */
export function makeProductHoldingQueries(cfg: ProductHoldingQueriesConfig) {
  const listKey = `${cfg.prefix}-products`
  const entryKey = `${cfg.prefix}-product-entry`
  const countsKey = `${cfg.prefix}-product-counts`

  function useProductsQuery(game: Ref<string>, page: Ref<number>) {
    const options = {
      queryKey: [listKey, game, page],
      queryFn: (token: string) =>
        cfg.getList(token, game.value, {
          page: page.value,
          pageSize: PRODUCT_HOLDING_PAGE_SIZE,
        }),
      placeholderData: keepPreviousData,
    }
    return useAuthedQuery<ProductHoldingPage>(options)
  }

  /** A page of the user's held sealed products grouped by set (`total` counts sets). The
   * key sits under the same `[listKey, game]` prefix as the flat list, so `invalidate`'s
   * prefix invalidation of `[listKey, game]` already refreshes it after any product write. */
  function useProductsBySetQuery(game: Ref<string>, page: Ref<number>) {
    const options = {
      queryKey: [listKey, game, 'by-set', page],
      queryFn: (token: string) =>
        cfg.getListBySet(token, game.value, {
          page: page.value,
          pageSize: PRODUCT_HOLDING_SET_PAGE_SIZE,
        }),
      placeholderData: keepPreviousData,
    }
    return useAuthedQuery<ProductHoldingSetPage>(options)
  }

  function useEntryQuery(
    game: Ref<string>,
    id: Ref<string>,
    opts: { enabled?: Ref<boolean>; staleTime?: number } = {},
  ) {
    const options = {
      queryKey: [entryKey, game, id],
      queryFn: (token: string) => cfg.getEntry(token, game.value, id.value),
      enabled: opts.enabled,
      staleTime: opts.staleTime,
    }
    return useAuthedQuery<CollectionQuantities>(options)
  }

  function useSummaryQuery(game: Ref<string>) {
    const options = {
      queryKey: [listKey, game, 'summary'],
      queryFn: (token: string) => cfg.getSummary(token, game.value),
    }
    return useAuthedQuery<ProductHoldingSummary>(options)
  }

  function useCounts(game: Ref<string>, products: Ref<{ id: string }[]>) {
    return useBatchCounts(countsKey, cfg.getCounts, game, products)
  }

  function invalidate(qc: QueryClient, game: string, opts: { entryId?: string } = {}) {
    qc.invalidateQueries({ queryKey: [listKey, game] })
    qc.invalidateQueries({ queryKey: [countsKey, game] })
    qc.invalidateQueries({
      queryKey: opts.entryId ? [entryKey, game, opts.entryId] : [entryKey, game],
    })
    if (cfg.invalidateAnalytics) {
      qc.invalidateQueries({ queryKey: ['collection-value-history', game] })
      qc.invalidateQueries({ queryKey: ['collection-movers', game] })
    }
  }

  function useSetEntryMutation() {
    const qc = useQueryClient()
    const options = {
      mutationFn: (token: string, vars: SetHoldingVars) =>
        cfg.setEntry(token, vars.game, vars.id, {
          quantity: vars.quantity,
          foil_quantity: vars.foil_quantity,
        }),
      onSuccess: (data: CollectionQuantities, vars: SetHoldingVars) => {
        qc.setQueryData([entryKey, vars.game, vars.id], data)
      },
      onSettled: (
        _data: CollectionQuantities | undefined,
        _error: ApiError | null,
        vars: SetHoldingVars,
      ) => {
        invalidate(qc, vars.game, { entryId: vars.id })
      },
    }
    return useAuthedMutation<CollectionQuantities, SetHoldingVars>(options)
  }

  return {
    useProductsQuery,
    useProductsBySetQuery,
    useEntryQuery,
    useSummaryQuery,
    useCounts,
    invalidate,
    useSetEntryMutation,
  }
}
