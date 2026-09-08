import { API_URL, request } from './client'
import type { PriceRange } from './catalog'
import type {
  Page,
  Product,
  ProductCardEntry,
  ProductCardSection,
  ProductContainer,
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
  OpenedCard,
  OpenedPack,
  PackCardOdds,
  PackEv,
  PackOpening,
  Product,
  ProductCardEntry,
  ProductCardSection,
  ProductComponent,
  ProductContainer,
  ProductEv,
  ProductFacets,
  ProductPricePoint,
  ProductPrices,
  ProductSetRef,
  SealedProductRef,
  SlotEv,
} from './generated'

import type { PackOpening, ProductComponent, ProductEv, SealedProductRef } from './generated'

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
export function listProducts(
  game: string,
  params?: ProductListParams,
  signal?: AbortSignal,
): Promise<ProductPage> {
  return request<ProductPage>(
    `/api/games/${encodeURIComponent(game)}/products${productQuery(params)}`,
    { signal },
  )
}

/** One sealed product by id. */
export function getProduct(game: string, id: string): Promise<Product> {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  return request<Product>(`/api/games/${g}/products/${i}`)
}

/**
 * The sealed product's structural composition — "what's in the box": nested packs/boxes
 * (each linked to its own product), precon decks, fixed promo cards (linked to the card),
 * and physical extras, in display order. Each component carries its `kind`, display `name`,
 * `quantity`, and — when it resolves to a catalog product/card — the linked `product`/`card`.
 * Empty when the product has no ingested composition.
 */
export function getProductContents(
  game: string,
  id: string,
  signal?: AbortSignal,
): Promise<{ data: ProductComponent[] }> {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  return request<{ data: ProductComponent[] }>(`/api/games/${g}/products/${i}/contents`, {
    signal,
  })
}

/**
 * The sealed products whose direct structural composition includes this product. Each
 * entry embeds the parent product and reports how many copies of the viewed product it
 * contains. Empty when this product is not linked from any ingested composition.
 */
export function getProductContainers(
  game: string,
  id: string,
  signal?: AbortSignal,
): Promise<{ data: ProductContainer[] }> {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  return request<{ data: ProductContainer[] }>(`/api/games/${g}/products/${i}/containers`, {
    signal,
  })
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
 * selected section. Pass `component` (a `component` value from the sections manifest) to
 * page the cards packed in one **unlisted** box component instead — `section` then narrows
 * within that component's cards. Pass `q` to narrow the page to the product's cards
 * matching a Scryfall-style search (the same grammar the card catalog accepts — name
 * substrings plus `c:r`, `t:goblin`, `r:mythic`, …), applied on top of `section` (issue
 * #222). Pass `sort`/`dir` (the shared card-list sort vocabulary) to re-order the cards
 * *within* each section; omit them for the product's natural membership / set-number order.
 */
export function getProductCards(
  game: string,
  id: string,
  page = 1,
  pageSize?: number,
  section?: ProductCardSectionKey,
  component?: string,
  q?: string,
  sort?: string,
  dir?: string,
  signal?: AbortSignal,
): Promise<ProductCardsPage> {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  const search = new URLSearchParams()
  if (page > 1) search.set('page', String(page))
  if (pageSize) search.set('page_size', String(pageSize))
  if (section) search.set('section', section)
  if (component) search.set('component', component)
  if (q) search.set('q', q)
  if (sort) search.set('sort', sort)
  if (dir) search.set('dir', dir)
  const qs = search.toString()
  return request<ProductCardsPage>(`/api/games/${g}/products/${i}/cards${qs ? `?${qs}` : ''}`, {
    signal,
  })
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
  signal?: AbortSignal,
): Promise<{ data: ProductCardSection[] }> {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  const qs = q ? `?q=${encodeURIComponent(q)}` : ''
  return request<{ data: ProductCardSection[] }>(
    `/api/games/${g}/products/${i}/cards/sections${qs}`,
    { signal },
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

// ---------- Booster expected value + the seeded pack opener (issue #682) ----------
//
// Two public reads over the same booster-sheet data: what an average pack is worth, and
// what one *seeded* roll of the dice actually dealt. Both are anonymous catalog reads.

/**
 * The expected value of one copy of a sealed product at today's prices, or `data: null`
 * when the product has no booster sheets to open (a precon deck, a product MTGJSON
 * doesn't describe) — the SPA renders nothing at all for a `null`.
 */
export function getProductEv(
  game: string,
  id: string,
  signal?: AbortSignal,
): Promise<{ data: ProductEv | null }> {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  return request<{ data: ProductEv | null }>(`/api/games/${g}/products/${i}/ev`, { signal })
}

/** How many copies {@link openProduct} may be asked for at once, and the API's own ceiling
 * on the packs one request may deal (a request past it is a 422). */
export const MAX_OPENING_COPIES = 6
export const MAX_OPENING_PACKS = 36

/** What to open: a client-minted u32 `seed` (always sent, so the response is a pure
 * function of its URL and can be shared, replayed and CDN-cached) and how many copies of
 * the product to open. */
export interface OpenProductParams {
  seed: number
  copies?: number
}

/**
 * Simulate opening `copies` of a sealed product with the given seed. The response is
 * deterministic in `(id, seed, copies)` — pack *n* is the same pack whatever `copies` was
 * — so the caller can mirror the seed into its URL and reproduce the run exactly.
 *
 * `422` when the product has no booster data, when `copies` is 0, or when the request
 * would deal more than the API's pack/card ceilings.
 */
export function openProduct(
  game: string,
  id: string,
  params: OpenProductParams,
  signal?: AbortSignal,
): Promise<PackOpening> {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  const search = new URLSearchParams({ seed: String(params.seed) })
  if (params.copies && params.copies > 1) search.set('copies', String(params.copies))
  return request<PackOpening>(`/api/games/${g}/products/${i}/open?${search.toString()}`, { signal })
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
