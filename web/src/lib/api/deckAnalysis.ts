import { request } from './client'
import type {
  DeckAnalytics,
  DeckBracketEstimate,
  DeckFormat,
  DeckLegality,
  DeckManaBase,
  DeckPricing,
  DeckRoles,
  DeckSuggestions,
  DeckTokens,
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
  DeckCheapestPrinting,
  DeckComposition,
  DeckDrawOdds,
  DeckFormat,
  DeckFormatGroup,
  DeckIssueStatus,
  DeckLegality,
  DeckLegalityIssue,
  DeckManaBase,
  DeckManaColor,
  DeckManaDemandCard,
  DeckManaSource,
  DeckManaStatus,
  DeckPricing,
  DeckPricingLine,
  DeckRole,
  DeckRoleCard,
  DeckRoleGroup,
  DeckRoles,
  DeckRuleCardStatus,
  DeckRuleId,
  DeckRuleSeverity,
  DeckRuleViolation,
  DeckStatItem,
  DeckSuggestionCard,
  DeckSuggestionRole,
  DeckSuggestions,
  DeckToken,
  DeckTokenSource,
  DeckTokens,
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
/** The catalog's published decklists — addressed by slug, since a precon has no deck row and
 *  its table ids are re-minted on every sync. Anonymous, like every other precon read. */
const preconBase = (game: string, slug: string): string =>
  `/api/games/${encodeURIComponent(game)}/precons/${encodeURIComponent(slug)}`

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

// ----- Tokens -----

/** The tokens and emblems a deck's cards make — what to bring to a game besides the deck. */
export function getDeckTokens(token: string, game: string, deckId: number): Promise<DeckTokens> {
  return request<DeckTokens>(`${deckBase(game, deckId)}/tokens`, { token })
}

/** The same read for a deck its owner shared. */
export function getPublicDeckTokens(handle: string, deckId: number): Promise<DeckTokens> {
  return request<DeckTokens>(`${publicBase(handle, deckId)}/tokens`)
}

// ----- Roles -----

/** What each of a deck's cards does — ramp, draw, removal and the rest — read off its rules
 * text server-side, so a CLI gets the same eight buckets the deck page draws. */
export function getDeckRoles(token: string, game: string, deckId: number): Promise<DeckRoles> {
  return request<DeckRoles>(`${deckBase(game, deckId)}/roles`, { token })
}

/** The same read for a deck its owner shared. */
export function getPublicDeckRoles(handle: string, deckId: number): Promise<DeckRoles> {
  return request<DeckRoles>(`${publicBase(handle, deckId)}/roles`)
}

// ----- Pricing -----

/** Where a deck's value is (issue #672): every row of the deck proper priced as held, most
 * expensive first, each with the cheapest priced printing of its card at the row's own finish
 * split and the saving a swap would make; plus the totals. `total_usd` is the detail's own
 * `summary.total_value_usd`; `null` anywhere means unpriced, never `$0.00`. */
export function getDeckPricing(token: string, game: string, deckId: number): Promise<DeckPricing> {
  return request<DeckPricing>(`${deckBase(game, deckId)}/pricing`, { token })
}

/** The cards in the caller's collection the deck could play (issue #684): legal in its
 * format, inside its colour identity, not already in it, most popular first and grouped by
 * role. Owner-only — it reads the caller's collection, so there is no public mirror. */
export function getDeckSuggestions(
  token: string,
  game: string,
  deckId: number,
): Promise<DeckSuggestions> {
  return request<DeckSuggestions>(`${deckBase(game, deckId)}/suggestions`, { token })
}

/** The same read for a deck its owner shared. */
export function getPublicDeckPricing(handle: string, deckId: number): Promise<DeckPricing> {
  return request<DeckPricing>(`${publicBase(handle, deckId)}/pricing`)
}

// ----- Mana base -----

/** The deck's colour pips against its sources, judged by Karsten's source counts (issue
 * #670): per colour, what the spells demand, what the library produces, the number a deck
 * this size needs for its hungriest spell, and a plain verdict. */
export function getDeckMana(token: string, game: string, deckId: number): Promise<DeckManaBase> {
  return request<DeckManaBase>(`${deckBase(game, deckId)}/mana`, { token })
}

/** The same read for a deck its owner shared. */
export function getPublicDeckMana(handle: string, deckId: number): Promise<DeckManaBase> {
  return request<DeckManaBase>(`${publicBase(handle, deckId)}/mana`)
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

// ----- Preconstructed decks -----
//
// The third address for the same reads. The server computes them with the very same
// core it uses for a deck, so these return byte-identical payloads to the two above — which
// is what lets the deck panels render a precon unchanged.

/** Composition + draw odds for a published decklist. */
export function getPreconStats(
  game: string,
  slug: string,
  params: DeckStatsParams = {},
): Promise<DeckAnalytics> {
  return request<DeckAnalytics>(`${preconBase(game, slug)}/stats${statsQuery(params)}`)
}

/** A precon's legality verdict, or `null` when its deck *type* states no format (a Jumpstart
 *  theme, an intro pack) — most precons, so a null here is the common case, not a failure. */
export function getPreconLegality(
  game: string,
  slug: string,
): Promise<{ data: DeckLegality | null }> {
  return request<{ data: DeckLegality | null }>(`${preconBase(game, slug)}/legality`)
}

/** A precon's estimated Commander bracket, or `null` outside Commander. */
export function getPreconBracket(
  game: string,
  slug: string,
): Promise<{ data: DeckBracketEstimate | null }> {
  return request<{ data: DeckBracketEstimate | null }>(`${preconBase(game, slug)}/bracket`)
}

/** The tokens a published decklist makes — the ones its product's token sheet holds. */
export function getPreconTokens(game: string, slug: string): Promise<DeckTokens> {
  return request<DeckTokens>(`${preconBase(game, slug)}/tokens`)
}

/** The roles a published decklist's cards fill. */
export function getPreconRoles(game: string, slug: string): Promise<DeckRoles> {
  return request<DeckRoles>(`${preconBase(game, slug)}/roles`)
}

/** A published decklist's mana base — judged against the deck size its type states. */
export function getPreconMana(game: string, slug: string): Promise<DeckManaBase> {
  return request<DeckManaBase>(`${preconBase(game, slug)}/mana`)
}

/** A seeded sample hand from a published decklist. */
export function getPreconGoldfish(
  game: string,
  slug: string,
  params: GoldfishParams = {},
): Promise<GoldfishHand> {
  return request<GoldfishHand>(`${preconBase(game, slug)}/goldfish${goldfishQuery(params)}`)
}
