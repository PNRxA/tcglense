import { API_URL, request } from './client'
import type { PriceRange } from './catalog'
import type { Page, Product, ProductFacets, ProductPricePoint } from './generated'

// ---------- Sealed products (public, game-agnostic) ----------
//
// The sealed-product section: booster boxes, bundles, decks, … tracked with the same
// price-over-time treatment cards get. The wire types (`Product`, `ProductFacets`,
// `ProductPricePoint`) are generated from the API's Rust DTOs into `./generated` and
// re-exported here so importers keep the `@/lib/api` entrypoint.

export type {
  Product,
  ProductFacets,
  ProductPricePoint,
  ProductPrices,
  ProductSetRef,
} from './generated'

/** A page of sealed products plus pagination cursors. */
export type ProductPage = Page<Product>

/** The product image proxy only serves two sizes (there is no `large`/`png`/`art_crop`
 * as for cards). */
export type ProductImageSize = 'normal' | 'small'

/** Reactive list controls for the product-browse view. Unlike the card lists, `q` is a
 * plain name substring (not Scryfall syntax); `set`/`type` are equality filters. */
export interface ProductListParams {
  page?: number
  pageSize?: number
  q?: string
  /** Restrict to one set (its `set_code`). */
  set?: string
  /** Restrict to one product type (a classifier slug like `collector_display`). */
  type?: string
  /** Sort field: `name`/`price`/`released`. */
  sort?: string
  /** Sort direction: `asc`/`desc`. */
  dir?: string
}

/** Encode the product-list query params, skipping falsy values, in a fixed order. */
function productQuery(params: ProductListParams = {}): string {
  const search = new URLSearchParams()
  if (params.page) search.set('page', String(params.page))
  if (params.pageSize) search.set('page_size', String(params.pageSize))
  if (params.q) search.set('q', params.q)
  if (params.set) search.set('set', params.set)
  if (params.type) search.set('type', params.type)
  if (params.sort) search.set('sort', params.sort)
  if (params.dir) search.set('dir', params.dir)
  const qs = search.toString()
  return qs ? `?${qs}` : ''
}

/** A page of a game's sealed products (name search + set/type filters + sort). */
export function listProducts(game: string, params?: ProductListParams): Promise<ProductPage> {
  return request<ProductPage>(
    `/api/games/${encodeURIComponent(game)}/products${productQuery(params)}`,
  )
}

/** One sealed product by id. */
export function getProduct(game: string, id: string): Promise<Product> {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  return request<Product>(`/api/games/${g}/products/${i}`)
}

/** The distinct product types + sets that actually have products, for filter dropdowns. */
export function getProductFacets(game: string): Promise<{ data: ProductFacets }> {
  return request<{ data: ProductFacets }>(`/api/games/${encodeURIComponent(game)}/products/facets`)
}

/**
 * Relative `/api/...` path for a product's price history, with an optional `range`
 * (same window vocabulary + downsampling as the card price history). Returns a path
 * (not an absolute URL) — `request()` prepends the API origin.
 */
export function productPriceHistoryPath(game: string, id: string, range?: PriceRange): string {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  const qs = range ? `?range=${encodeURIComponent(range)}` : ''
  return `/api/games/${g}/products/${i}/prices${qs}`
}

/** Price history for a product, oldest first (empty array if no rows recorded yet). */
export function getProductPrices(
  game: string,
  id: string,
  range?: PriceRange,
): Promise<{ data: ProductPricePoint[] }> {
  return request<{ data: ProductPricePoint[] }>(productPriceHistoryPath(game, id, range))
}

/** URL of the caching image proxy for a product, for `<img src>`. */
export function productImageUrl(
  game: string,
  id: string,
  size: ProductImageSize = 'normal',
): string {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  return `${API_URL}/api/games/${g}/products/${i}/image?size=${size}`
}
