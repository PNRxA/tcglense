import { useQuery, type QueryClient } from '@tanstack/vue-query'
import type { Ref } from 'vue'
import {
  getDeckGoldfish,
  getDeckLegality,
  getDeckStats,
  getPublicDeckGoldfish,
  getPublicDeckLegality,
  getPublicDeckStats,
  type DeckStatsParams,
  type GoldfishParams,
} from '@/lib/api/deckAnalysis'
import type { ApiError, DeckAnalytics, DeckLegality, GoldfishHand } from '@/lib/api'
import { useAuthedQuery } from '@/lib/queries'

// ---------- Deck analysis queries (issue #596) ----------
//
// Composition, legality, and goldfish, each in an authed and a handle-addressed public
// form — the surface a component is on is fixed at mount, so a dual-mode component selects
// once (`props.handle ? usePublicX() : useX()`), exactly as `ProductHoldingSection` does.
//
// Reactive params go INSIDE the query key as refs, so changing the library selection or the
// goldfish's seed/mulligans/draws refetches. That is also what makes the goldfish cheap to
// step through: each state is its own cache entry, so stepping back to a hand you have
// already seen is instant, and it is the same URL a CLI would call.

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
 * every previously dealt hand for that deck a hand of a deck that no longer exists.
 */
export function invalidateDeckAnalysis(qc: QueryClient, game: string, deckId?: number) {
  const keys =
    deckId === undefined
      ? [
          ['deck-stats', game],
          ['deck-legality', game],
          ['deck-goldfish', game],
        ]
      : [
          ['deck-stats', game, deckId],
          ['deck-legality', game, deckId],
          ['deck-goldfish', game, deckId],
        ]
  for (const queryKey of keys) qc.invalidateQueries({ queryKey })
}
