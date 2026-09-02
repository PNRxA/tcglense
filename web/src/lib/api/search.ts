import { request } from './client'
import type { SearchResults } from './generated'

// ---------- Universal search (public) ----------
//
// One `q` answered across cards, sealed products, preconstructed decks and the keyword
// glossary at once — the homepage search box's read. The wire shape is generated from the
// API's `SearchResults` DTO; each group carries the same row type its own listing does
// (`Card`, `Product`, `PreconDeck`, `KeywordEntry`), so the tiles already built for those
// render a hit unchanged. The signed-in user's own decks are NOT part of this read — it is
// the same for every visitor, which is what keeps it CDN-cacheable — the search composable
// adds them from the deck list it already holds.

export type { SearchGroup, SearchResults } from './generated'

/** The universal search path for a query, with an optional per-group cap (the API clamps
 * it to 1–10 and defaults to 5). */
export function searchPath(game: string, q: string, limit?: number): string {
  const search = new URLSearchParams({ q })
  if (limit) search.set('limit', String(limit))
  return `/api/games/${encodeURIComponent(game)}/search?${search}`
}

export function searchCatalog(
  game: string,
  q: string,
  limit?: number,
  signal?: AbortSignal,
): Promise<SearchResults> {
  return request<SearchResults>(searchPath(game, q, limit), { signal })
}
