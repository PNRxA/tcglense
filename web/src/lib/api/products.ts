import { API_URL, request } from './client'
import type { PriceRange } from './catalog'
import type { Page, Product, ProductCardEntry, ProductFacets, ProductPricePoint } from './generated'

// ---------- Sealed products (public, game-agnostic) ----------
//
// The sealed-product section: booster boxes, bundles, decks, … tracked with the same
// price-over-time treatment cards get. The wire types (`Product`, `ProductFacets`,
// `ProductPricePoint`) are generated from the API's Rust DTOs into `./generated` and
// re-exported here so importers keep the `@/lib/api` entrypoint.

export type {
  Product,
  ProductCardEntry,
  ProductFacets,
  ProductPricePoint,
  ProductPrices,
  ProductSetRef,
  SealedProductRef,
} from './generated'

import type { SealedProductRef } from './generated'

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

/**
 * The sealed products a card is found in / can be pulled from — each entry carries the
 * product plus its `membership` bucket (`contains` / `booster` / `variable`, the
 * "found in / can be in / may be in" split) and a `foil` flag. Ordered `contains` →
 * `booster` → `variable`, then by product name. Empty when the card is in none.
 */
export function getCardSealed(game: string, id: string): Promise<{ data: SealedProductRef[] }> {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  return request<{ data: SealedProductRef[] }>(`/api/games/${g}/cards/${i}/sealed`)
}

/** A page of the cards a sealed product contains / can be pulled from, plus cursors. */
export type ProductCardsPage = Page<ProductCardEntry>

/**
 * The cards a sealed product is found to contain — or can be pulled from — the reverse
 * of {@link getCardSealed}. Each entry carries the card plus its `membership` bucket
 * (`contains` / `booster` / `variable`) and a `foil`-only flag. Ordered `contains` →
 * `booster` → `variable` (guaranteed cards lead), then by set + collector number, and
 * paginated by card. Empty page when the product has no ingested contents.
 */
export function getProductCards(
  game: string,
  id: string,
  page = 1,
  pageSize?: number,
): Promise<ProductCardsPage> {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  const search = new URLSearchParams()
  if (page > 1) search.set('page', String(page))
  if (pageSize) search.set('page_size', String(pageSize))
  const qs = search.toString()
  return request<ProductCardsPage>(`/api/games/${g}/products/${i}/cards${qs ? `?${qs}` : ''}`)
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
