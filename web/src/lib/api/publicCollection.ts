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
import type { CollectionSet, CollectionSummary, PublicProfile } from './generated'

// ---------- Public collections (read-only, unauthenticated) ----------
//
// A read-only view of another user's owned cards for a game they've made public (issues
// #361/#362), addressed by their handle (`{username}-{discriminator}`). No token — the URL
// (handle + game) fully identifies the content, so these ride the shared CDN cache. A
// private/unknown handle or game comes back as a 404 (`ApiError`), never confirming a
// handle exists or a game is merely hidden. Wire types are generated from the API's Rust
// DTOs into `./generated`.

export type { PublicGameSummary, PublicProfile } from './generated'

const base = (handle: string) => `/api/u/${encodeURIComponent(handle)}`

/** A user's public profile: identity + a summary per game they've made public. */
export function getPublicProfile(handle: string): Promise<PublicProfile> {
  return request<PublicProfile>(base(handle))
}

/** A page of a user's public collection for a game (most-recently-updated first). */
export function getPublicCollection(
  handle: string,
  game: string,
  params?: CollectionListParams,
): Promise<CollectionPage> {
  return request<CollectionPage>(
    `${base(handle)}/${encodeURIComponent(game)}${listQuery(params ?? {})}`,
  )
}

/** Aggregate stats (unique cards, total copies, estimated value) for a user's public
 * collection in a game — optionally scoped to one set (and, with `includeRelated`, that
 * set's whole group), so the browse view's scoped value/completion line matches the authed
 * one. `bulkMaxCents` sets the cutoff the server splits the bulk subtotal at (the viewer's
 * own bulk-threshold preference, in cents); omitted = the server default ($1). */
export function getPublicCollectionSummary(
  handle: string,
  game: string,
  opts: { set?: string; includeRelated?: boolean; bulkMaxCents?: number } = {},
): Promise<CollectionSummary> {
  // include_related only means anything alongside a set scope (matches the backend).
  const qs = listQuery({
    set: opts.set,
    includeRelated: opts.set ? opts.includeRelated : undefined,
    bulkMaxCents: opts.bulkMaxCents,
  })
  return request<CollectionSummary>(`${base(handle)}/${encodeURIComponent(game)}/summary${qs}`)
}

/** The sets the user owns cards in for a public game — the per-set landing tiles, each
 * dressed with catalog metadata + owned counts (mirrors the authed `getCollectionSets`). */
export function getPublicCollectionSets(
  handle: string,
  game: string,
): Promise<{ data: CollectionSet[] }> {
  return request<{ data: CollectionSet[] }>(`${base(handle)}/${encodeURIComponent(game)}/sets`)
}

/** A page (by Secret Lair drop) of a user's public collection in a drop-grouped set —
 * the show-ghosts / by-drop mirror of the authed `getCollectionSetDrops`. */
export function getPublicCollectionDrops(
  handle: string,
  game: string,
  code: string,
  params?: CollectionDropsParams,
): Promise<CollectionDropGroupPage> {
  const g = encodeURIComponent(game)
  const c = encodeURIComponent(code)
  return request<CollectionDropGroupPage>(
    `${base(handle)}/${g}/sets/${c}/drops${listQuery(params ?? {})}`,
  )
}

/** A page (by card sub-type / treatment) of a user's public collection in a set —
 * mirrors the authed `getCollectionSetSubtypes`. */
export function getPublicCollectionSubtypes(
  handle: string,
  game: string,
  code: string,
  params?: CollectionDropsParams,
): Promise<CollectionSubtypeGroupPage> {
  const g = encodeURIComponent(game)
  const c = encodeURIComponent(code)
  return request<CollectionSubtypeGroupPage>(
    `${base(handle)}/${g}/sets/${c}/subtypes${listQuery(params ?? {})}`,
  )
}

/** Which of the given catalog card ids the owner holds, keyed by external id (cards they
 * don't own are simply absent) — the show-ghosts overlay on the public browse grid.
 * Token-less POST to `.../owned`, batched under the server id cap like the authed
 * `getCollectionOwned`. */
export function getPublicOwnedCounts(
  handle: string,
  game: string,
  ids: string[],
): Promise<OwnedCountsMap> {
  return postCountsBatched(`${base(handle)}/${encodeURIComponent(game)}/owned`, null, ids)
}
