import { listQuery, request } from './client'
import type { CollectionListParams, CollectionPage } from './collection'
import type { CollectionSummary, PublicProfile } from './generated'

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
 * collection in a game. */
export function getPublicCollectionSummary(
  handle: string,
  game: string,
): Promise<CollectionSummary> {
  return request<CollectionSummary>(`${base(handle)}/${encodeURIComponent(game)}/summary`)
}
