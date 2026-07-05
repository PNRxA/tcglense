import { API_URL, request } from './client'
import type { PriceRange } from './catalog'
import type {
  Page,
  Product,
  ProductCardEntry,
  ProductCardSection,
  ProductFacets,
  ProductPricePoint,
} from './generated'

// ---------- Sealed products (public, game-agnostic) ----------
//
// The sealed-product section: booster boxes, bundles, decks, … tracked with the same
// price-over-time treatment cards get. The wire types (`Product`, `ProductFacets`,
// `ProductPricePoint`) are generated from the API's Rust DTOs into `./generated` and
// re-exported here so importers keep the `@/lib/api` entrypoint.

export type {
  Product,
  ProductCardEntry,
  ProductCardSection,
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

/** A display section a product's cards split into (`contains` / `exclusive` / `booster` /
 * `variable`) — the `?section=` filter value {@link getProductCards} pages within. */
export type ProductCardSectionKey = 'contains' | 'exclusive' | 'booster' | 'variable'

/**
 * The cards a sealed product is found to contain — or can be pulled from — the reverse
 * of {@link getCardSealed}. Each entry carries the card plus its `membership` bucket
 * (`contains` / `booster` / `variable`) and a `foil`-only flag. Ordered `contains` →
 * `booster` → `variable` (guaranteed cards lead), then by set + collector number, and
 * paginated by card. Empty page when the product has no ingested contents.
 *
 * Pass `section` to page just one display section (each rendered with its own pagination,
 * issue #224); omit it for the whole ordered list. `total`/`has_more` then describe the
 * selected section. Pass `q` to narrow the page to the product's cards matching a
 * Scryfall-style search (the same grammar the card catalog accepts — name substrings plus
 * `c:r`, `t:goblin`, `r:mythic`, …), applied on top of `section` (issue #222).
 */
export function getProductCards(
  game: string,
  id: string,
  page = 1,
  pageSize?: number,
  section?: ProductCardSectionKey,
  q?: string,
): Promise<ProductCardsPage> {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  const search = new URLSearchParams()
  if (page > 1) search.set('page', String(page))
  if (pageSize) search.set('page_size', String(pageSize))
  if (section) search.set('section', section)
  if (q) search.set('q', q)
  const qs = search.toString()
  return request<ProductCardsPage>(`/api/games/${g}/products/${i}/cards${qs ? `?${qs}` : ''}`)
}

/**
 * The non-empty display sections of a product's cards, in display order (`contains` →
 * `exclusive` → `booster` → `variable`), each with its card count — the manifest the SPA
 * reads first to know which sections exist (and how big) before paginating each on its own
 * with {@link getProductCards}'s `section` param (issue #224). Empty when the product has
 * no ingested contents. Pass the same `q` search as {@link getProductCards} to get the
 * filtered manifest — only the sections (with recomputed counts) whose cards match, so the
 * blocks the SPA renders line up with the filtered pages (issue #222).
 */
export function getProductCardSections(
  game: string,
  id: string,
  q?: string,
): Promise<{ data: ProductCardSection[] }> {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  const qs = q ? `?q=${encodeURIComponent(q)}` : ''
  return request<{ data: ProductCardSection[] }>(
    `/api/games/${g}/products/${i}/cards/sections${qs}`,
  )
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
