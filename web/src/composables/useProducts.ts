import { type Ref } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { getProduct, getProductFacets, listProducts } from '@/lib/api'
import { toSortParam } from '@/lib/cardSort'

/**
 * Shared reads for the sealed-products section. These are PUBLIC catalog endpoints
 * (no auth), so they use plain `useQuery` — not the `useAuthedQuery` wrapper — and
 * carry their reactive params inside the query key so a change refetches.
 */

/** Sealed products per page in the browse grid. */
export const PRODUCT_PAGE_SIZE = 60

/** Reactive controls for the product-list query, all carried in the key so a change
 * refetches; `defaultSort` backs an empty sort. */
interface ProductListQueryOptions {
  page: Ref<number>
  query: Ref<string>
  set: Ref<string>
  type: Ref<string>
  sort: Ref<string>
  defaultSort: string
}

/** A page of a game's sealed products (name search + set/type filters + sort).
 * `keepPreviousData` holds the current grid up while the next page loads. */
export function useProductsQuery(game: Ref<string>, opts: ProductListQueryOptions) {
  return useQuery({
    queryKey: ['products', game, opts.query, opts.set, opts.type, opts.sort, opts.page],
    queryFn: () =>
      listProducts(game.value, {
        q: opts.query.value || undefined,
        set: opts.set.value || undefined,
        type: opts.type.value || undefined,
        page: opts.page.value,
        pageSize: PRODUCT_PAGE_SIZE,
        ...toSortParam(opts.sort.value, opts.defaultSort),
      }),
    placeholderData: keepPreviousData,
  })
}

/** One sealed product by id. Keyed on `['product', game, id]`. */
export function useProductQuery(game: Ref<string>, id: Ref<string>) {
  return useQuery({
    queryKey: ['product', game, id],
    queryFn: () => getProduct(game.value, id.value),
  })
}

/** The distinct product types + sets that have products, for the filter dropdowns.
 * Effectively static per game, so it stays fresh a while. */
export function useProductFacetsQuery(game: Ref<string>) {
  return useQuery({
    queryKey: ['product-facets', game],
    queryFn: () => getProductFacets(game.value),
    staleTime: 60 * 60 * 1000,
  })
}
