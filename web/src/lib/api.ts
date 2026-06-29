// Empty base -> relative '/api/...' URLs, which go through the Vite dev proxy in
// dev and are same-origin in production. Override with VITE_API_URL if needed.
const API_URL = import.meta.env.VITE_API_URL ?? ''

export interface User {
  id: number
  email: string
  display_name: string | null
  created_at: string
}

export interface AuthResponse {
  access_token: string
  user: User
}

export interface RefreshResponse {
  access_token: string
}

export interface RegisterPayload {
  email: string
  password: string
  display_name?: string | null
}

export interface LoginPayload {
  email: string
  password: string
}

/** Error thrown for non-2xx responses, carrying the server message and HTTP status. */
export class ApiError extends Error {
  readonly status: number

  constructor(message: string, status: number) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }
}

interface RequestOptions {
  method?: string
  body?: unknown
  token?: string | null
}

async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
  }
  if (options.token) {
    headers.Authorization = `Bearer ${options.token}`
  }

  const response = await fetch(`${API_URL}${path}`, {
    method: options.method ?? 'GET',
    headers,
    // Always send/receive the httpOnly refresh cookie (tcglense_refresh).
    credentials: 'include',
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
  })

  // 204 No Content and other empty bodies have nothing to parse; tolerate
  // non-JSON bodies (e.g. a proxy error page) rather than throwing on parse.
  const text = await response.text()
  let data: unknown = null
  if (text) {
    try {
      data = JSON.parse(text)
    } catch {
      data = null
    }
  }

  if (!response.ok) {
    const error = (data as { error?: unknown } | null)?.error
    const message =
      typeof error === 'string' ? error : `Request failed with status ${response.status}`
    throw new ApiError(message, response.status)
  }

  return (data ?? undefined) as T
}

export function register(payload: RegisterPayload): Promise<AuthResponse> {
  return request<AuthResponse>('/api/auth/register', { method: 'POST', body: payload })
}

export function login(payload: LoginPayload): Promise<AuthResponse> {
  return request<AuthResponse>('/api/auth/login', { method: 'POST', body: payload })
}

export function me(token: string): Promise<{ user: User }> {
  return request<{ user: User }>('/api/auth/me', { token })
}

export function refresh(): Promise<RefreshResponse> {
  return request<RefreshResponse>('/api/auth/refresh', { method: 'POST' })
}

export function logout(): Promise<void> {
  return request<void>('/api/auth/logout', { method: 'POST' })
}

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
}

export interface CardFace {
  name: string | null
  mana_cost: string | null
  type_line: string | null
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
  color_identity: string[]
  colors: string[]
  layout: string | null
  prices: CardPrices
  has_image: boolean
  faces: CardFace[]
}

/** A page of cards plus pagination cursors. */
export interface CardPage {
  data: Card[]
  page: number
  page_size: number
  total: number
  has_more: boolean
}

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
}

function cardQuery(params: CardListParams = {}): string {
  const search = new URLSearchParams()
  if (params.page) search.set('page', String(params.page))
  if (params.pageSize) search.set('page_size', String(params.pageSize))
  if (params.q) search.set('q', params.q)
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

export function getCard(game: string, id: string): Promise<Card> {
  return request<Card>(`/api/games/${encodeURIComponent(game)}/cards/${encodeURIComponent(id)}`)
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
