import { computed, ref, type ComputedRef, type Ref } from 'vue'
import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/vue-query'
import {
  getCard,
  getSet,
  listCards,
  listGames,
  listSetCards,
  listSetDrops,
  listSets,
} from '@/lib/api'
import { toSortParam } from '@/lib/cardSort'
import { findCardInCache, findSetInCache } from '@/lib/placeholders'

/**
 * Shared catalog reads. The game registry and per-game set list are used across the
 * catalog nav + views, so they're centralised here to share one query key (and thus
 * one warm cache entry) rather than being re-declared inline in each consumer. The
 * card-list reads are shared too: the collection's show-ghosts mode renders exactly
 * the catalog lists, so it reuses these hooks (and their cache entries) rather than
 * re-declaring the same fetches under different keys.
 */

/** Cards per page in the flat card grids (catalog and collection alike). */
export const CARD_PAGE_SIZE = 60

/** Drops per page in the by-drop views — they paginate over *drops* (each a handful
 * of cards), so they use a smaller page size than the flat card grid. */
export const DROP_PAGE_SIZE = 20

/** Reactive list controls shared by the card-list queries: `page`/`query`/`sort` are
 * carried in the query key so a change refetches; `defaultSort` backs an empty sort. */
interface CardListQueryOptions {
  page: Ref<number>
  query: Ref<string>
  sort: Ref<string>
  defaultSort: string
  enabled?: Ref<boolean>
}

/** The supported-games registry. Effectively static, so it never goes stale. */
export function useGamesQuery() {
  return useQuery({
    queryKey: ['games'],
    queryFn: () => listGames(),
    staleTime: Infinity,
  })
}

/** A game's display name from the (cached) registry, falling back to its
 * upper-cased id until the registry loads or for an unknown game. */
export function useGameName(game: Ref<string>): ComputedRef<string> {
  const gamesQuery = useGamesQuery()
  return computed(
    () =>
      gamesQuery.data.value?.data.find((g) => g.id === game.value)?.name ??
      game.value.toUpperCase(),
  )
}

/** A game's full set list. Keyed on `['sets', game]` so GameView, SetView and the
 * grouping composable all read the same warm entry. */
export function useSetsQuery(game: Ref<string>) {
  return useQuery({
    queryKey: ['sets', game],
    queryFn: ({ signal }) => listSets(game.value, signal),
  })
}

/** One set's metadata. Keyed on `['set', game, code]` so the catalog and collection
 * set views share one warm entry. */
export function useSetQuery(game: Ref<string>, code: Ref<string>, enabled?: Ref<boolean>) {
  const qc = useQueryClient()
  return useQuery({
    queryKey: ['set', game, code],
    queryFn: () => getSet(game.value, code.value),
    // Seed from the (warm) set-list cache so the set view paints instantly; the real fetch
    // still runs (placeholderData, not initialData) and reads the current refs so a
    // set→set navigation re-evaluates.
    placeholderData: () => findSetInCache(qc, game.value, code.value),
    enabled,
  })
}

/** One card's full detail. Keyed on `['card', game, id]` so the browse-grid modal
 * (CardDetailContent) and the standalone page (CardDetailView) share one warm entry and
 * never double-fetch. Seeds from a warm list/prints/holdings cache so opening a card from
 * a grid paints instantly (placeholderData, not initialData — the real fetch still runs
 * and reads the current refs so a card→card navigation re-evaluates). */
export function useCardQuery(game: Ref<string>, id: Ref<string>) {
  const qc = useQueryClient()
  return useQuery({
    queryKey: ['card', game, id],
    queryFn: () => getCard(game.value, id.value),
    placeholderData: () => findCardInCache(qc, game.value, id.value),
  })
}

/** A page of one set's cards (searchable + sortable, optionally spanning the set's
 * related group). `keepPreviousData` keeps the current grid up while the next page
 * loads (smoother paging). */
export function useSetCardsQuery(
  game: Ref<string>,
  code: Ref<string>,
  opts: CardListQueryOptions & { includeRelated?: Ref<boolean> },
) {
  // Fall back to a stable "not grouped" ref so the query key is well-formed either way.
  const includeRelated = opts.includeRelated ?? ref(false)
  return useQuery({
    queryKey: ['set-cards', game, code, opts.query, opts.sort, opts.page, includeRelated],
    queryFn: ({ signal }) =>
      listSetCards(
        game.value,
        code.value,
        {
          q: opts.query.value || undefined,
          page: opts.page.value,
          pageSize: CARD_PAGE_SIZE,
          includeRelated: includeRelated.value || undefined,
          ...toSortParam(opts.sort.value, opts.defaultSort),
        },
        signal,
      ),
    enabled: opts.enabled,
    placeholderData: keepPreviousData,
  })
}

/** A page of a game's whole card list (searchable + sortable). */
export function useAllCardsQuery(game: Ref<string>, opts: CardListQueryOptions) {
  return useQuery({
    queryKey: ['cards', game, opts.query, opts.sort, opts.page],
    queryFn: ({ signal }) =>
      listCards(
        game.value,
        {
          q: opts.query.value || undefined,
          page: opts.page.value,
          pageSize: CARD_PAGE_SIZE,
          ...toSortParam(opts.sort.value, opts.defaultSort),
        },
        signal,
      ),
    enabled: opts.enabled,
    placeholderData: keepPreviousData,
  })
}

/** A page (by drop) of a drop-grouped set's cards, grouped into Secret Lair drops.
 * The caller gates it on the by-drop view being active via `opts.enabled` (the
 * endpoint 404s for a set without drops — see `CardSet.has_drops`). `query` narrows
 * the cards within each drop; the optional `drop` narrows the drops by their curated
 * title (SetView's "filter drops by name" box — the show-ghosts by-drop views omit
 * it). Both are reactive and ride the query key. */
export function useSetDropsQuery(
  game: Ref<string>,
  code: Ref<string>,
  opts: { page: Ref<number>; query: Ref<string>; drop?: Ref<string>; enabled?: Ref<boolean> },
) {
  // Fall back to a stable empty ref so the query key is well-formed whether or not the
  // caller wires the drop-title filter (mirrors useSetCardsQuery's includeRelated).
  const drop = opts.drop ?? ref('')
  return useQuery({
    queryKey: ['set-drops', game, code, opts.query, drop, opts.page],
    queryFn: ({ signal }) =>
      listSetDrops(
        game.value,
        code.value,
        {
          q: opts.query.value || undefined,
          drop: drop.value || undefined,
          page: opts.page.value,
          pageSize: DROP_PAGE_SIZE,
        },
        signal,
      ),
    enabled: opts.enabled,
    placeholderData: keepPreviousData,
  })
}
