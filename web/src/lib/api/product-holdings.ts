import { listQuery, request } from './client'
import { postCountsBatched } from './holdings'
import type { OwnedCountsMap } from './collection'
import type {
  CollectionQuantities,
  Page,
  ProductHoldingEntry,
  ProductHoldingSet,
  ProductHoldingSummary,
} from './generated'

export type ProductHoldingTarget = 'collection' | 'wishlist'
export type ProductHoldingPage = Page<ProductHoldingEntry>

/**
 * Build the sealed-product holding client shared by collection and wish list. The two
 * surfaces have independent storage but the same wire contract; only the URL base and
 * batch-count leaf differ (`collection/.../owned`, `wishlist/.../counts`).
 */
export function makeProductHoldingApi(base: ProductHoldingTarget, countsLeaf: 'owned' | 'counts') {
  // The flat list optionally scopes to one set (`?set=<code>`, recency order kept), which the
  // set-scoped browse view passes through; without it the whole holding is listed.
  const productsPath = (
    game: string,
    params: { page?: number; pageSize?: number; set?: string } = {},
  ): string => `/api/${base}/${encodeURIComponent(game)}/products${listQuery(params)}`

  const setsPath = (game: string): string =>
    `/api/${base}/${encodeURIComponent(game)}/products/sets`

  const entryPath = (game: string, id: string): string =>
    `/api/${base}/${encodeURIComponent(game)}/products/${encodeURIComponent(id)}`

  const summaryPath = (game: string): string =>
    `/api/${base}/${encodeURIComponent(game)}/products/summary`

  const countsPath = (game: string): string =>
    `/api/${base}/${encodeURIComponent(game)}/products/${countsLeaf}`

  return {
    productsPath,
    setsPath,
    entryPath,
    summaryPath,
    countsPath,
    list(
      token: string,
      game: string,
      params?: { page?: number; pageSize?: number; set?: string },
    ): Promise<ProductHoldingPage> {
      return request<ProductHoldingPage>(productsPath(game, params), { token })
    },
    listSets(token: string, game: string): Promise<{ data: ProductHoldingSet[] }> {
      return request<{ data: ProductHoldingSet[] }>(setsPath(game), { token })
    },
    getEntry(token: string, game: string, id: string): Promise<CollectionQuantities> {
      return request<CollectionQuantities>(entryPath(game, id), { token })
    },
    setEntry(
      token: string,
      game: string,
      id: string,
      body: CollectionQuantities,
    ): Promise<CollectionQuantities> {
      return request<CollectionQuantities>(entryPath(game, id), {
        method: 'PUT',
        body,
        token,
      })
    },
    summary(token: string, game: string): Promise<ProductHoldingSummary> {
      return request<ProductHoldingSummary>(summaryPath(game), { token })
    },
    counts(token: string, game: string, ids: string[]): Promise<OwnedCountsMap> {
      return postCountsBatched(countsPath(game), token, ids)
    },
  }
}
