import { API_URL, request } from './client'

// ---------- Card catalog (public, game-agnostic) ----------

/** A supported trading-card game. */
export interface Game {
  id: string
  name: string
  publisher: string
  data_source: string
}

/** A set/expansion within a game. */
export interface CardSet {
  code: string
  name: string
  set_type: string | null
  released_at: string | null
  card_count: number
  icon_svg_uri: string | null
  parent_set_code: string | null
  /** Whether this set can be browsed broken down into Secret Lair-style "drops"
   * (the `listSetDrops` endpoint). */
  has_drops: boolean
}

export interface CardFace {
  name: string | null
  mana_cost: string | null
  type_line: string | null
  oracle_text: string | null
  power: string | null
  toughness: string | null
  loyalty: string | null
}

export interface CardPrices {
  usd: string | null
  usd_foil: string | null
  eur: string | null
  tix: string | null
}

/** A single printing of a card. */
export interface Card {
  id: string
  name: string
  set_code: string
  set_name: string
  collector_number: string
  rarity: string | null
  lang: string
  released_at: string | null
  mana_cost: string | null
  cmc: number | null
  type_line: string | null
  oracle_text: string | null
  power: string | null
  toughness: string | null
  loyalty: string | null
  color_identity: string[]
  colors: string[]
  layout: string | null
  prices: CardPrices
  has_image: boolean
  /** Secret Lair drop title this card belongs to (for drop-grouped sets); null otherwise. */
  drop_name: string | null
  /** Slug of the drop above, paired with `drop_name`. */
  drop_slug: string | null
  faces: CardFace[]
}

/** A page of items plus pagination cursors — the shape every paginated list endpoint
 * returns. Reused by the collection API for its own paged responses. */
export interface Page<T> {
  data: T[]
  page: number
  page_size: number
  total: number
  has_more: boolean
}

/** A page of cards plus pagination cursors. */
export type CardPage = Page<Card>

/** A Secret Lair drop: a named group of cards (e.g. "Wild in Bloom"). */
export interface DropGroup {
  /** Stable slug for anchors; null for the catch-all "Other" group. */
  slug: string | null
  title: string
  card_count: number
  cards: Card[]
}

/** A page of drop groups — `total`/pagination count *drops*, not cards. */
export type DropGroupPage = Page<DropGroup>

/** Background import status for a game's card data. */
export interface IngestStatus {
  status: string
  detail: string | null
  sets_imported: number
  cards_imported: number
  source_updated_at: string | null
  finished_at: string | null
}

export type ImageSize = 'small' | 'normal' | 'large' | 'png' | 'art_crop'

export interface CardListParams {
  page?: number
  pageSize?: number
  q?: string
  /** Set-cards only: span the set's whole group (root + related sub-sets). */
  includeRelated?: boolean
  /** Sort field: `number`/`name`/`rarity`/`released`/`cmc`/`price`. */
  sort?: string
  /** Sort direction: `asc`/`desc`. */
  dir?: string
  /** All-cards endpoint only: restrict to printings whose name matches this exactly
   * (the quick-add "pick a printing of this name" step). Ignored by the set-cards
   * endpoint. */
  name?: string
}

function cardQuery(params: CardListParams = {}): string {
  const search = new URLSearchParams()
  if (params.page) search.set('page', String(params.page))
  if (params.pageSize) search.set('page_size', String(params.pageSize))
  if (params.q) search.set('q', params.q)
  if (params.includeRelated) search.set('include_related', 'true')
  if (params.sort) search.set('sort', params.sort)
  if (params.dir) search.set('dir', params.dir)
  if (params.name) search.set('name', params.name)
  const qs = search.toString()
  return qs ? `?${qs}` : ''
}

export function listGames(): Promise<{ data: Game[] }> {
  return request<{ data: Game[] }>('/api/games')
}

export function gameStatus(game: string): Promise<IngestStatus> {
  return request<IngestStatus>(`/api/games/${encodeURIComponent(game)}/status`)
}

export function listSets(game: string): Promise<{ data: CardSet[] }> {
  return request<{ data: CardSet[] }>(`/api/games/${encodeURIComponent(game)}/sets`)
}

export function getSet(game: string, code: string): Promise<CardSet> {
  return request<CardSet>(`/api/games/${encodeURIComponent(game)}/sets/${encodeURIComponent(code)}`)
}

export function listSetCards(
  game: string,
  code: string,
  params?: CardListParams,
): Promise<CardPage> {
  const g = encodeURIComponent(game)
  const c = encodeURIComponent(code)
  return request<CardPage>(`/api/games/${g}/sets/${c}/cards${cardQuery(params)}`)
}

export function listCards(game: string, params?: CardListParams): Promise<CardPage> {
  return request<CardPage>(`/api/games/${encodeURIComponent(game)}/cards${cardQuery(params)}`)
}

/** How many name hints the quick-add box requests (the server also caps this). */
export const CARD_NAME_SUGGESTION_LIMIT = 10

/** Relative `/api/games/{game}/card-names` path for the quick-add autocomplete. */
export function cardNamesPath(game: string, q: string, limit = CARD_NAME_SUGGESTION_LIMIT): string {
  const search = new URLSearchParams({ q, limit: String(limit) })
  return `/api/games/${encodeURIComponent(game)}/card-names?${search.toString()}`
}

/** Distinct card names matching `q` (one hint per unique name) for the collection
 * quick-add box; empty when `q` is blank. */
export function getCardNames(
  game: string,
  q: string,
  limit = CARD_NAME_SUGGESTION_LIMIT,
): Promise<{ data: string[] }> {
  return request<{ data: string[] }>(cardNamesPath(game, q, limit))
}

/** Upper bound on printings fetched for one name (the all-cards endpoint caps a
 * page at 200 — comfortably more than any non-basic card's printing count). */
export const MAX_PRINTINGS = 200

/** Every printing of an exact card `name` in a game, newest printing first — the
 * quick-add "pick which printing" step. Reuses the all-cards endpoint's exact-name
 * filter, so no card-id is needed (the name comes straight from the autocomplete). */
export function getCardPrintingsByName(game: string, name: string): Promise<CardPage> {
  return listCards(game, { name, pageSize: MAX_PRINTINGS, sort: 'released', dir: 'desc' })
}

/** Browse a drop-grouped set (e.g. Secret Lair) broken into named drops,
 * paginated by drop. Only valid for sets where `CardSet.has_drops` is true. */
export function listSetDrops(
  game: string,
  code: string,
  params?: Pick<CardListParams, 'page' | 'pageSize' | 'q'>,
): Promise<DropGroupPage> {
  const g = encodeURIComponent(game)
  const c = encodeURIComponent(code)
  return request<DropGroupPage>(`/api/games/${g}/sets/${c}/drops${cardQuery(params)}`)
}

export function getCard(game: string, id: string): Promise<Card> {
  return request<Card>(`/api/games/${encodeURIComponent(game)}/cards/${encodeURIComponent(id)}`)
}

/** A card's other printings (every card sharing its gameplay identity/oracle id),
 * newest printing first. Empty when the card has no other printings. */
export function getCardPrints(game: string, id: string): Promise<{ data: Card[] }> {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  return request<{ data: Card[] }>(`/api/games/${g}/cards/${i}/prints`)
}

/** A single day's recorded prices for a card (decimal strings, exactly as stored). */
export interface PricePoint {
  date: string
  usd: string | null
  usd_foil: string | null
  eur: string | null
  tix: string | null
}

/**
 * Window + resolution for the price-history chart. Longer ranges are downsampled
 * server-side to a coarser resolution; omitting it returns the full daily series.
 */
export type PriceRange = '7d' | '30d' | '1y' | '2y' | '3y' | 'all'

/**
 * Relative `/api/...` path for a card's price history, with an optional `range`.
 * Returns a path (not an absolute URL) — `request()` prepends the API origin, so
 * this must not include it (unlike `cardImageUrl`, which is used as a bare `src`).
 */
export function priceHistoryPath(game: string, id: string, range?: PriceRange): string {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  const qs = range ? `?range=${encodeURIComponent(range)}` : ''
  return `/api/games/${g}/cards/${i}/prices${qs}`
}

/** Price history for a card, oldest first (empty array if no rows recorded yet). */
export function getPriceHistory(
  game: string,
  id: string,
  range?: PriceRange,
): Promise<{ data: PricePoint[] }> {
  return request<{ data: PricePoint[] }>(priceHistoryPath(game, id, range))
}

/** URL of the caching proxy for a set's SVG icon, for `<img src>`. */
export function setIconUrl(game: string, code: string): string {
  const g = encodeURIComponent(game)
  const c = encodeURIComponent(code)
  return `${API_URL}/api/games/${g}/sets/${c}/icon`
}

/** URL of the caching image proxy for a card (and optional face), for `<img src>`. */
export function cardImageUrl(
  game: string,
  id: string,
  size: ImageSize = 'normal',
  face?: number,
): string {
  const g = encodeURIComponent(game)
  const i = encodeURIComponent(id)
  const faceParam = face === undefined ? '' : `&face=${face}`
  return `${API_URL}/api/games/${g}/cards/${i}/image?size=${size}${faceParam}`
}
