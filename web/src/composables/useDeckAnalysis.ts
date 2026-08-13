import { keepPreviousData, useQuery, type QueryClient } from '@tanstack/vue-query'
import type { Ref } from 'vue'
import { PRICED_CATALOG_STALE_MS } from '@/lib/queryClient'
import {
  getPreconBracket,
  getPreconGoldfish,
  getPreconLegality,
  getPreconStats,
  getPreconTokens,
  getDeckBracket,
  getDeckGoldfish,
  getDeckLegality,
  getDeckStats,
  getDeckTokens,
  getPublicDeckBracket,
  getPublicDeckGoldfish,
  getPublicDeckLegality,
  getPublicDeckStats,
  getPublicDeckTokens,
  type DeckStatsParams,
  type GoldfishParams,
} from '@/lib/api/deckAnalysis'
import type {
  ApiError,
  DeckAnalytics,
  DeckBracketEstimate,
  DeckLegality,
  DeckTokens,
  GoldfishHand,
} from '@/lib/api'
import { useAuthedQuery } from '@/lib/queries'

// ---------- Deck analysis queries (issue #596) ----------
//
// Composition, legality, the estimated Commander bracket, and goldfish, each in an authed
// and a handle-addressed public form — the surface a component is on is fixed at mount, so a
// dual-mode component selects once (`props.handle ? usePublicX() : useX()`), exactly as
// `ProductHoldingSection` does.
//
// Reactive params go INSIDE the query key as refs, so changing the library selection or the
// goldfish's seed/mulligans/draws refetches. That is also what makes the goldfish cheap to
// step through: each state is its own cache entry, so stepping back to a hand you have
// already seen is instant, and it is the same URL a CLI would call.//
// Every one of these keeps its previous response while a new one is in flight
// (`placeholderData: keepPreviousData`). Without it, changing any control blanks `data` for
// the round trip, which tears down the panel the user is mid-click in — a regression the
// pre-server-side version couldn't have, since it computed from props.

// ----- Authed reads (the caller's own deck) -----

/** A deck's composition and draw odds. */
export function useDeckStatsQuery(
  game: Ref<string>,
  deckId: Ref<number>,
  params: Ref<DeckStatsParams>,
  enabled?: Ref<boolean>,
) {
  const options = {
    queryKey: ['deck-stats', game, deckId, params],
    queryFn: (token: string) => getDeckStats(token, game.value, deckId.value, params.value),
    enabled,
    placeholderData: keepPreviousData,
  }
  return useAuthedQuery<DeckAnalytics>(options)
}

/** A deck's legality verdict (`data` is null for an untracked format). */
export function useDeckLegalityQuery(
  game: Ref<string>,
  deckId: Ref<number>,
  enabled?: Ref<boolean>,
) {
  const options = {
    queryKey: ['deck-legality', game, deckId],
    queryFn: (token: string) => getDeckLegality(token, game.value, deckId.value),
    enabled,
  }
  return useAuthedQuery<{ data: DeckLegality | null }>(options)
}

/** A Commander deck's estimated bracket (`data` is null for any other format). */
export function useDeckBracketQuery(
  game: Ref<string>,
  deckId: Ref<number>,
  enabled?: Ref<boolean>,
) {
  const options = {
    queryKey: ['deck-bracket', game, deckId],
    queryFn: (token: string) => getDeckBracket(token, game.value, deckId.value),
    enabled,
  }
  return useAuthedQuery<{ data: DeckBracketEstimate | null }>(options)
}

/** The tokens a deck makes. */
export function useDeckTokensQuery(game: Ref<string>, deckId: Ref<number>, enabled?: Ref<boolean>) {
  const options = {
    queryKey: ['deck-tokens', game, deckId],
    queryFn: (token: string) => getDeckTokens(token, game.value, deckId.value),
    enabled,
    placeholderData: keepPreviousData,
  }
  return useAuthedQuery<DeckTokens>(options)
}

/** A goldfished hand. */
export function useDeckGoldfishQuery(
  game: Ref<string>,
  deckId: Ref<number>,
  params: Ref<GoldfishParams>,
  enabled?: Ref<boolean>,
) {
  const options = {
    queryKey: ['deck-goldfish', game, deckId, params],
    queryFn: (token: string) => getDeckGoldfish(token, game.value, deckId.value, params.value),
    enabled,
    placeholderData: keepPreviousData,
    // A hand is a pure function of its parameters, so once fetched it can never go stale.
    staleTime: Infinity,
  }
  return useAuthedQuery<GoldfishHand>(options)
}

// ----- Public reads (a deck its owner shared) -----

/** A public deck's composition and draw odds. `retry: false` so a 404 is terminal. */
export function usePublicDeckStatsQuery(
  handle: Ref<string>,
  deckId: Ref<number>,
  params: Ref<DeckStatsParams>,
  enabled?: Ref<boolean>,
) {
  return useQuery<DeckAnalytics, ApiError>({
    queryKey: ['public-deck-stats', handle, deckId, params],
    queryFn: () => getPublicDeckStats(handle.value, deckId.value, params.value),
    enabled,
    retry: false,
    placeholderData: keepPreviousData,
  })
}

/** A public deck's legality verdict. */
export function usePublicDeckLegalityQuery(
  handle: Ref<string>,
  deckId: Ref<number>,
  enabled?: Ref<boolean>,
) {
  return useQuery<{ data: DeckLegality | null }, ApiError>({
    queryKey: ['public-deck-legality', handle, deckId],
    queryFn: () => getPublicDeckLegality(handle.value, deckId.value),
    enabled,
    retry: false,
  })
}

/** A public deck's estimated bracket. */
export function usePublicDeckBracketQuery(
  handle: Ref<string>,
  deckId: Ref<number>,
  enabled?: Ref<boolean>,
) {
  return useQuery<{ data: DeckBracketEstimate | null }, ApiError>({
    queryKey: ['public-deck-bracket', handle, deckId],
    queryFn: () => getPublicDeckBracket(handle.value, deckId.value),
    enabled,
    retry: false,
  })
}

/** The tokens a public deck makes. */
export function usePublicDeckTokensQuery(
  handle: Ref<string>,
  deckId: Ref<number>,
  enabled?: Ref<boolean>,
) {
  return useQuery<DeckTokens, ApiError>({
    queryKey: ['public-deck-tokens', handle, deckId],
    queryFn: () => getPublicDeckTokens(handle.value, deckId.value),
    enabled,
    retry: false,
    placeholderData: keepPreviousData,
  })
}

/** A hand goldfished from a public deck — the same seed deals the same cards as it would
 * for the owner. */
export function usePublicDeckGoldfishQuery(
  handle: Ref<string>,
  deckId: Ref<number>,
  params: Ref<GoldfishParams>,
  enabled?: Ref<boolean>,
) {
  return useQuery<GoldfishHand, ApiError>({
    queryKey: ['public-deck-goldfish', handle, deckId, params],
    queryFn: () => getPublicDeckGoldfish(handle.value, deckId.value, params.value),
    enabled,
    retry: false,
    placeholderData: keepPreviousData,
    staleTime: Infinity,
  })
}

// ----- Invalidation -----

/**
 * Drop a deck's analysis after an edit. Its own key family, because the analysis keys don't
 * sit under `['deck', …]` and would otherwise survive a card change — a deck page showing
 * last edit's mana curve is worse than one that refetches.
 *
 * The goldfish goes too: its cards come from the library, so a card added or removed makes
 * every previously dealt hand for that deck a hand of a deck that no longer exists. So does
 * the bracket: adding one Game Changer is exactly the edit that moves it — and so do the
 * tokens, since the card just added may be the only one that made one.
 */
export function invalidateDeckAnalysis(qc: QueryClient, game: string, deckId?: number) {
  const keys =
    deckId === undefined
      ? [
          ['deck-stats', game],
          ['deck-legality', game],
          ['deck-bracket', game],
          ['deck-tokens', game],
          ['deck-goldfish', game],
        ]
      : [
          ['deck-stats', game, deckId],
          ['deck-legality', game, deckId],
          ['deck-bracket', game, deckId],
          ['deck-tokens', game, deckId],
          ['deck-goldfish', game, deckId],
        ]
  for (const queryKey of keys) qc.invalidateQueries({ queryKey })
}

// ----- Preconstructed decks (published catalog decklists) -----
//
// A third address for the same four reads. Their own key prefix on purpose: a precon is
// immutable catalog data rebuilt only by the daily sync, so it must NOT be swept by
// `invalidateDeckAnalysis` when an unrelated deck is edited — and a slug would otherwise
// share a key space with a numeric deck id. `staleTime` matches the rest of the precon
// surface for the same reason.

/** A precon's composition and draw odds. */
export function usePreconStatsQuery(
  game: Ref<string>,
  slug: Ref<string>,
  params: Ref<DeckStatsParams>,
  enabled?: Ref<boolean>,
) {
  return useQuery<DeckAnalytics, ApiError>({
    queryKey: ['precon-stats', game, slug, params],
    queryFn: () => getPreconStats(game.value, slug.value, params.value),
    enabled,
    retry: false,
    staleTime: PRICED_CATALOG_STALE_MS,
    placeholderData: keepPreviousData,
  })
}

/** A precon's legality verdict — `null` for the ~9 in 10 whose type states no format. */
export function usePreconLegalityQuery(
  game: Ref<string>,
  slug: Ref<string>,
  enabled?: Ref<boolean>,
) {
  return useQuery<{ data: DeckLegality | null }, ApiError>({
    queryKey: ['precon-legality', game, slug],
    queryFn: () => getPreconLegality(game.value, slug.value),
    enabled,
    retry: false,
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}

/** A precon's estimated Commander bracket — `null` outside Commander. */
export function usePreconBracketQuery(
  game: Ref<string>,
  slug: Ref<string>,
  enabled?: Ref<boolean>,
) {
  return useQuery<{ data: DeckBracketEstimate | null }, ApiError>({
    queryKey: ['precon-bracket', game, slug],
    queryFn: () => getPreconBracket(game.value, slug.value),
    enabled,
    retry: false,
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}

/** The tokens a published decklist makes. */
export function usePreconTokensQuery(game: Ref<string>, slug: Ref<string>, enabled?: Ref<boolean>) {
  return useQuery<DeckTokens, ApiError>({
    queryKey: ['precon-tokens', game, slug],
    queryFn: () => getPreconTokens(game.value, slug.value),
    enabled,
    retry: false,
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}

/** A seeded sample hand from a precon. Not `staleTime`d like its siblings: the hand is a
 *  function of its params, and those change as the player mulligans. */
export function usePreconGoldfishQuery(
  game: Ref<string>,
  slug: Ref<string>,
  params: Ref<GoldfishParams>,
  enabled?: Ref<boolean>,
) {
  return useQuery<GoldfishHand, ApiError>({
    queryKey: ['precon-goldfish', game, slug, params],
    queryFn: () => getPreconGoldfish(game.value, slug.value, params.value),
    enabled,
    retry: false,
    placeholderData: keepPreviousData,
  })
}
