import { request } from './client'
import type {
  DeckDetail,
  Page,
  PreconDeck,
  PreconDeckDetail,
  PreconFacets,
  PreconGroup,
} from './generated'

// ---------- Preconstructed decks (public catalog + one authed write) ----------
//
// The decklists a publisher shipped: Commander decks, Planeswalker / Challenger / Starter
// decks, Jumpstart themes, Secret Lair drops — derived from MTGJSON during the sealed sync.
// They're *catalog* data, not the user's, so the list/facets/detail reads take no token and
// live beside `products` rather than `decks`. The one write — copying a precon into your own
// decks — is authed and returns a normal `DeckDetail`, so the caller navigates to the deck
// page it just created exactly as the public-deck copy does.
//
// A precon is addressed by its **slug** (`turtle-power-tmc`), never an id: the tables are
// rebuilt wholesale on every sync, so ids are re-minted while a slug is stable.

export type { PreconCardEntry, PreconDeck, PreconDeckDetail, PreconFacets } from './generated'
export type { PreconFaceCard, PreconGroup, PreconSetRef, PreconTypeRef } from './generated'

/** A page of precon decks plus pagination cursors. */
export type PreconPage = Page<PreconDeck>

/** A page of **groups**, each with its precon decks — the grouped views' payload. Paginated by
 *  group, so a group's decks are never split across a page boundary. */
export type PreconGroupPage = Page<PreconGroup>

/** What a grouped listing buckets by: the set that published the decks, or the deck type. */
export type PreconGrouping = 'set' | 'type'

/** Reactive list controls for the precon browse view. Like the sealed list, `q` is a plain
 * name substring (not Scryfall syntax) and `set`/`type` are equality filters. */
export interface PreconListParams {
  page?: number
  pageSize?: number
  q?: string
  /** Restrict to one set (its `set_code`). */
  set?: string
  /** With `set`, span its whole catalog group (root + related sub-sets) instead of the one
   *  code — the precon mirror of the card listing's own related view. */
  includeRelated?: boolean
  /** Restrict to one deck type, e.g. `Commander Deck` (see the facets endpoint). */
  type?: string
  /** `released` (default, newest first) or `name`. */
  sort?: string
  /** Grouped listings only: `set` (default) or `type`. */
  group?: PreconGrouping
}

/** Encode the precon-list query params, skipping falsy values, in a fixed order. */
function preconQuery(params: PreconListParams = {}): string {
  const search = new URLSearchParams()
  if (params.page) search.set('page', String(params.page))
  if (params.pageSize) search.set('page_size', String(params.pageSize))
  if (params.q) search.set('q', params.q)
  if (params.set) search.set('set', params.set)
  // Only meaningful alongside a set, and the server ignores it otherwise — but keep it off the
  // URL entirely there, so an unscoped listing has one canonical (cacheable) query string.
  if (params.set && params.includeRelated) search.set('include_related', 'true')
  if (params.type) search.set('type', params.type)
  if (params.sort) search.set('sort', params.sort)
  if (params.group) search.set('group', params.group)
  const qs = search.toString()
  return qs ? `?${qs}` : ''
}

const base = (game: string): string => `/api/games/${encodeURIComponent(game)}/precons`

/** A page of a game's preconstructed decks (name search + set/type filters + sort). */
export function listPrecons(
  game: string,
  params?: PreconListParams,
  signal?: AbortSignal,
): Promise<PreconPage> {
  return request<PreconPage>(`${base(game)}${preconQuery(params)}`, { signal })
}

/** The same decks bucketed — by set (default) or by deck type — and paginated by group. */
export function listPreconGroups(
  game: string,
  params?: PreconListParams,
  signal?: AbortSignal,
): Promise<PreconGroupPage> {
  return request<PreconGroupPage>(`${base(game)}/groups${preconQuery(params)}`, { signal })
}

/** The deck types + sets that actually have precons, with counts — the filter vocabulary. */
export function getPreconFacets(
  game: string,
  signal?: AbortSignal,
): Promise<{
  data: PreconFacets
}> {
  return request<{ data: PreconFacets }>(`${base(game)}/facets`, { signal })
}

/** One precon in full: header, value summary, every card in board order, sealed product. */
export function getPrecon(game: string, slug: string): Promise<PreconDeckDetail> {
  return request<PreconDeckDetail>(`${base(game)}/${encodeURIComponent(slug)}`)
}

/** Copy a precon into the caller's own decks, returning the new deck's full detail. */
export function copyPrecon(token: string, game: string, slug: string): Promise<DeckDetail> {
  const g = encodeURIComponent(game)
  const s = encodeURIComponent(slug)
  return request<DeckDetail>(`/api/decks/${g}/precons/${s}/copy`, { method: 'POST', token })
}
