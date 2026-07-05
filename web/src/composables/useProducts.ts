import { type Ref } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import {
  getCardSealed,
  getProduct,
  getProductCards,
  getProductCardSections,
  getProductFacets,
  listProducts,
} from '@/lib/api'
import type { ProductCardSectionKey } from '@/lib/api'
import { toSortParam } from '@/lib/cardSort'

/**
 * Shared reads for the sealed-products section. These are PUBLIC catalog endpoints
 * (no auth), so they use plain `useQuery` — not the `useAuthedQuery` wrapper — and
 * carry their reactive params inside the query key so a change refetches.
 */

/** Sealed products per page in the browse grid. */
export const PRODUCT_PAGE_SIZE = 60

/** Cards per page in a product's "cards in this product" section. */
export const PRODUCT_CARDS_PAGE_SIZE = 60

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

/** A page of one display section of the cards a sealed product contains / can be pulled
 * from (the sealed-detail "Cards in this product" blocks — each section paginates on its
 * own, issue #224). Public read, so a plain `useQuery`; `keepPreviousData` holds the
 * current grid up while the next page loads. The `section` is fixed per block, so it sits
 * in the key as a plain value alongside the reactive `game`/`id`/`page`. */
export function useProductCardsQuery(
  game: Ref<string>,
  id: Ref<string>,
  page: Ref<number>,
  section: ProductCardSectionKey,
) {
  return useQuery({
    queryKey: ['product-cards', game, id, section, page],
    queryFn: () =>
      getProductCards(game.value, id.value, page.value, PRODUCT_CARDS_PAGE_SIZE, section),
    placeholderData: keepPreviousData,
  })
}

/** The non-empty display sections (+ counts) of a sealed product's cards — the manifest
 * driving which "Cards in this product" blocks to render (issue #224). Public read, so a
 * plain `useQuery`; refs go in the key so a product-to-product navigation refetches. */
export function useProductCardSectionsQuery(game: Ref<string>, id: Ref<string>) {
  return useQuery({
    queryKey: ['product-card-sections', game, id],
    queryFn: () => getProductCardSections(game.value, id.value),
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

/** The sealed products a card is found in / can be pulled from (the card-detail
 * "Sealed products" section). Public read, so a plain `useQuery`; refs go in the key so a
 * card-to-card navigation refetches. */
export function useCardSealedQuery(game: Ref<string>, id: Ref<string>) {
  return useQuery({
    queryKey: ['card-sealed', game, id],
    queryFn: () => getCardSealed(game.value, id.value),
  })
}
