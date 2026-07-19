import { listQuery, request } from './client'
import { postCountsBatched } from './holdings'
import type {
  CollectionDropGroupPage,
  CollectionDropsParams,
  CollectionListParams,
  CollectionPage,
  CollectionSubtypeGroupPage,
  OwnedCountsMap,
} from './collection'
import type { ProductHoldingPage } from './product-holdings'
import type {
  CollectionSet,
  CollectionSummary,
  ProductHoldingSet,
  ProductHoldingSummary,
} from './generated'

// ---------- Public wish lists (read-only, unauthenticated) — issue #493 ----------
//
// The wish-list twin of `publicCollection.ts`: a read-only view of another user's *wanted*
// cards + sealed products for a game they've made public, addressed by their handle under a
// static `wishlist` segment (`/api/u/{handle}/wishlist/{game}...`). No token — the URL fully
// identifies the content, so these ride the shared CDN cache. A private/unknown handle or game
// comes back as a 404 (`ApiError`), never confirming a handle exists or a wish list is merely
// hidden. Wire types are the collection's own (the backend reuses those DTOs).

const base = (handle: string, game: string) =>
  `/api/u/${encodeURIComponent(handle)}/wishlist/${encodeURIComponent(game)}`

/** A page of a user's public wish list for a game (most-recently-updated first). */
export function getPublicWishlist(
  handle: string,
  game: string,
  params?: CollectionListParams,
): Promise<CollectionPage> {
  return request<CollectionPage>(`${base(handle, game)}${listQuery(params ?? {})}`)
}

/** Aggregate stats (unique cards, total copies, estimated value) for a user's public wish
 * list in a game — optionally scoped to one set (and, with `includeRelated`, that set's whole
 * group). `bulkMaxCents` is accepted for signature parity with the collection summary, though
 * the wish-list landing never renders a bulk split. */
export function getPublicWishlistSummary(
  handle: string,
  game: string,
  opts: { set?: string; includeRelated?: boolean; bulkMaxCents?: number } = {},
): Promise<CollectionSummary> {
  const qs = listQuery({
    set: opts.set,
    includeRelated: opts.set ? opts.includeRelated : undefined,
    bulkMaxCents: opts.bulkMaxCents,
  })
  return request<CollectionSummary>(`${base(handle, game)}/summary${qs}`)
}

/** The sets the user wants cards in for a public wish list — the per-set landing tiles, each
 * dressed with catalog metadata + wanted counts (mirrors the authed `getWishlistSets`). */
export function getPublicWishlistSets(
  handle: string,
  game: string,
): Promise<{ data: CollectionSet[] }> {
  return request<{ data: CollectionSet[] }>(`${base(handle, game)}/sets`)
}

/** A page of a user's public wanted sealed products for a game, optionally scoped to one set. */
export function getPublicWishlistProducts(
  handle: string,
  game: string,
  params?: { page?: number; pageSize?: number; set?: string },
): Promise<ProductHoldingPage> {
  return request<ProductHoldingPage>(`${base(handle, game)}/products${listQuery(params ?? {})}`)
}

/** Aggregate stats for a user's public wanted sealed products in a game. */
export function getPublicWishlistProductSummary(
  handle: string,
  game: string,
): Promise<ProductHoldingSummary> {
  return request<ProductHoldingSummary>(`${base(handle, game)}/products/summary`)
}

/** The sets a user wants sealed products in for a public wish list — the per-set landing tiles. */
export function getPublicWishlistProductSets(
  handle: string,
  game: string,
): Promise<{ data: ProductHoldingSet[] }> {
  return request<{ data: ProductHoldingSet[] }>(`${base(handle, game)}/products/sets`)
}

/** A page (by Secret Lair drop) of a user's public wish list in a drop-grouped set. */
export function getPublicWishlistDrops(
  handle: string,
  game: string,
  code: string,
  params?: CollectionDropsParams,
): Promise<CollectionDropGroupPage> {
  const c = encodeURIComponent(code)
  return request<CollectionDropGroupPage>(
    `${base(handle, game)}/sets/${c}/drops${listQuery(params ?? {})}`,
  )
}

/** A page (by card sub-type / treatment) of a user's public wish list in a set. */
export function getPublicWishlistSubtypes(
  handle: string,
  game: string,
  code: string,
  params?: CollectionDropsParams,
): Promise<CollectionSubtypeGroupPage> {
  const c = encodeURIComponent(code)
  return request<CollectionSubtypeGroupPage>(
    `${base(handle, game)}/sets/${c}/subtypes${listQuery(params ?? {})}`,
  )
}

/** Which of the given catalog card ids the owner wants, keyed by external id (cards they
 * don't want are absent) — the show-ghosts overlay on the public wish-list browse grid. */
export function getPublicWishlistOwnedCounts(
  handle: string,
  game: string,
  ids: string[],
): Promise<OwnedCountsMap> {
  return postCountsBatched(`${base(handle, game)}/owned`, null, ids)
}
