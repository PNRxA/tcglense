import { request } from './client'
import type {
  DeckAnalytics,
  DeckBracketEstimate,
  DeckFormat,
  DeckLegality,
  GoldfishHand,
} from './generated'

// ---------- Deck analysis (issue #596) ----------
//
// Composition + draw odds, the legality verdict, and a goldfished sample hand. All three
// used to be computed in this bundle (`lib/deckStats.ts`, `lib/legality.ts`,
// `lib/deckRules.ts`); they now live on the deck surface so a CLI or an API key gets the
// same answers, and this module is only the transport. What stayed client-side is the
// *presentation* vocabulary — a status's label and colour, the format select's options —
// which is in `lib/legality.ts`.
//
// Every read has an authed form (the caller's own deck) and a handle-addressed public one
// (a deck its owner shared). The two hit different routes but return identical payloads,
// because the server drives both from one core.

export type {
  DeckAnalytics,
  DeckBracketCard,
  DeckBracketCategory,
  DeckBracketEstimate,
  DeckBracketLevel,
  DeckBracketSignal,
  DeckCardOdds,
  DeckComposition,
  DeckDrawOdds,
  DeckFormat,
  DeckFormatGroup,
  DeckIssueStatus,
  DeckLegality,
  DeckLegalityIssue,
  DeckRuleCardStatus,
  DeckRuleId,
  DeckRuleSeverity,
  DeckRuleViolation,
  DeckStatItem,
  GoldfishHand,
} from './generated'

/** Query parameters of the analytics read. `sections` omitted = the server's default
 * library (everything that isn't a maybeboard, command zone, or sideboard); an empty array
 * = explicitly none. `card` omitted = draw odds for the most-copied card. */
export interface DeckStatsParams {
  sections?: number[]
  card?: string
}

/** Query parameters of the goldfish read — the hand's entire state, which is why the same
 * URL always deals the same cards. */
export interface GoldfishParams {
  /** Shuffle seed. Omitted = the server picks one and echoes it back. */
  seed?: number
  /** How many times the hand was mulliganed (London: reshuffle, then bottom that many). */
  mulligans?: number
  /** External card ids put on the bottom, at most one per mulligan. */
  bottom?: string[]
  /** Cards drawn past the opening hand. */
  draws?: number
  /** Opening hand size (default 7). */
  opening?: number
  /** Sections to shuffle; omitted = the default library. */
  sections?: number[]
}

/** Build a query string, dropping `undefined` and keeping an explicitly empty list as an
 * empty value — `?sections=` means "no sections", which is not the same as omitting it. */
function queryString(params: Record<string, string | number | undefined>): string {
  const search = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined) search.set(key, String(value))
  }
  const query = search.toString()
  return query ? `?${query}` : ''
}

function statsQuery(params: DeckStatsParams): string {
  return queryString({
    sections: params.sections?.join(','),
    card: params.card,
  })
}

function goldfishQuery(params: GoldfishParams): string {
  return queryString({
    seed: params.seed,
    mulligans: params.mulligans,
    bottom: params.bottom?.length ? params.bottom.join(',') : undefined,
    draws: params.draws,
    opening: params.opening,
    sections: params.sections?.join(','),
  })
}

const deckBase = (game: string, deckId: number): string =>
  `/api/decks/${encodeURIComponent(game)}/${deckId}`
const publicBase = (handle: string, deckId: number): string =>
  `/api/u/${encodeURIComponent(handle)}/decks/${deckId}`

// ----- Analytics -----

/** Composition of a deck and of its shuffled library, plus the draw-odds curve. */
export function getDeckStats(
  token: string,
  game: string,
  deckId: number,
  params: DeckStatsParams = {},
): Promise<DeckAnalytics> {
  return request<DeckAnalytics>(`${deckBase(game, deckId)}/stats${statsQuery(params)}`, { token })
}

/** The same read for a deck its owner shared. */
export function getPublicDeckStats(
  handle: string,
  deckId: number,
  params: DeckStatsParams = {},
): Promise<DeckAnalytics> {
  return request<DeckAnalytics>(`${publicBase(handle, deckId)}/stats${statsQuery(params)}`)
}

// ----- Legality -----

/** A deck's legality verdict, or `null` when its format isn't one legality is tracked for
 * — which means "nothing to evaluate", never "illegal". */
export function getDeckLegality(
  token: string,
  game: string,
  deckId: number,
): Promise<{ data: DeckLegality | null }> {
  return request<{ data: DeckLegality | null }>(`${deckBase(game, deckId)}/legality`, { token })
}

/** The same read for a deck its owner shared. */
export function getPublicDeckLegality(
  handle: string,
  deckId: number,
): Promise<{ data: DeckLegality | null }> {
  return request<{ data: DeckLegality | null }>(`${publicBase(handle, deckId)}/legality`)
}

// ----- Bracket -----

/** A Commander deck's estimated bracket, or `null` when the deck isn't a Commander deck —
 * the one format Wizards' ladder is defined for. */
export function getDeckBracket(
  token: string,
  game: string,
  deckId: number,
): Promise<{ data: DeckBracketEstimate | null }> {
  return request<{ data: DeckBracketEstimate | null }>(`${deckBase(game, deckId)}/bracket`, {
    token,
  })
}

/** The same read for a deck its owner shared. */
export function getPublicDeckBracket(
  handle: string,
  deckId: number,
): Promise<{ data: DeckBracketEstimate | null }> {
  return request<{ data: DeckBracketEstimate | null }>(`${publicBase(handle, deckId)}/bracket`)
}

// ----- Goldfish -----

/** Deal a sample hand from a deck's library. */
export function getDeckGoldfish(
  token: string,
  game: string,
  deckId: number,
  params: GoldfishParams = {},
): Promise<GoldfishHand> {
  return request<GoldfishHand>(`${deckBase(game, deckId)}/goldfish${goldfishQuery(params)}`, {
    token,
  })
}

/** The same read for a deck its owner shared. */
export function getPublicDeckGoldfish(
  handle: string,
  deckId: number,
  params: GoldfishParams = {},
): Promise<GoldfishHand> {
  return request<GoldfishHand>(`${publicBase(handle, deckId)}/goldfish${goldfishQuery(params)}`)
}

// ----- Formats -----

/** The game's legality-tracked deck formats, in display order. Public and cacheable; the
 * SPA keeps its own copy for the format select (see `lib/legality.ts`) and this exists so
 * non-browser clients don't have to hard-code the list. */
export function getDeckFormats(game: string): Promise<{ data: DeckFormat[] }> {
  return request<{ data: DeckFormat[] }>(`/api/games/${encodeURIComponent(game)}/formats`)
}
