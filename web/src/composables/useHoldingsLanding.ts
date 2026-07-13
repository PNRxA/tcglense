import { computed, toRef, type Ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import type { UseQueryReturnType } from '@tanstack/vue-query'
import { useSetsQuery } from '@/composables/useCatalog'
import { useFilteredSetGroups } from '@/composables/useSetGrouping'
import { formatUsd } from '@/lib/money'
import { groupByYear, partitionPinned } from '@/lib/setGroups'
import type { ApiError, CardSet, CollectionSet, CollectionSummary } from '@/lib/api'
import type { CountNoun } from '@/lib/ownership'

/**
 * The per-holding hooks + labels the shared landing binds to. Collection and wish list
 * are landing-page twins (issue #167): they differ only by which summary/held-sets query
 * they read, the route prefix their tiles link under, the SetGroupGrid count noun, and
 * whether the per-set tiles carry a bulk-value slice — everything else (the URL scope
 * toggle, the filter + nested grouping, the featured/year sectioning, the ownership map,
 * the header stats) is identical. This bundles that per-surface config so both views drive
 * the same {@link useHoldingsLanding} composable.
 */
export interface HoldingLandingSurface {
  /** The holding's summary hook (`useCollectionSummaryQuery` / `useWishlistSummaryQuery`). */
  useSummaryQuery: (game: Ref<string>) => UseQueryReturnType<CollectionSummary, ApiError>
  /** The held-sets hook (`useCollectionSetsQuery` / `useWishlistSetsQuery`) — the sets the
   * user holds cards in, both the default mode's list and the per-set overlay. */
  useHeldSetsQuery: (game: Ref<string>) => UseQueryReturnType<{ data: CollectionSet[] }, ApiError>
  /** Route prefix the tiles link under (`/collection`, `/wishlist`, or a public
   * `/u/{handle}`). The engine never reads it — the view threads it into `SetGroupGrid` —
   * so it's a plain string. */
  basePath: string
  /** The SetGroupGrid count noun — `'wanted'` for the wish list, undefined (SetGroupGrid's
   * `'owned'` default) for the collection. */
  countNoun?: CountNoun
  /** Whether the per-set ownership object carries a bulk-value slice — the collection tiles
   * show one, the wish list's (a shopping list) don't. */
  withBulk: boolean
}

/**
 * The shared per-game holding landing: the logic behind GameCollectionView and
 * GameWishlistView, which differ only by the {@link HoldingLandingSurface} passed in. Owns
 * the `?sets=all` URL scope toggle, the mode-switched source set list (held sets by
 * default, the full catalog under "All sets"), the filter + nested grouping pipeline, the
 * featured/year sectioning, the per-set ownership map the tiles overlay, and the header
 * stats. Each view layers only its own surface-specific extras (the collection's sync
 * controls / camera scan / value-history chart / bulk-value stat) on top.
 */
export function useHoldingsLanding(props: { game: string }, surface: HoldingLandingSurface) {
  const game = toRef(props, 'game')

  const summaryQuery = surface.useSummaryQuery(game)
  // The sets holding the user's cards — the default mode's list and the per-set overlay
  // either mode shows.
  const heldSetsQuery = surface.useHeldSetsQuery(game)

  const summary = computed(() => summaryQuery.data.value)
  const heldSets = computed(() => heldSetsQuery.data.value?.data ?? [])

  // Which sets the grid lists: just the held ones (default) or the whole catalog
  // (`?sets=all`). A URL param, like the browse views' `?ghosts`, so the choice survives
  // navigation and the back button.
  const route = useRoute()
  const router = useRouter()
  const showAllSets = computed(() => route.query.sets === 'all')
  function setShowAllSets(on: boolean) {
    const next = { ...route.query }
    if (on) next.sets = 'all'
    else delete next.sets
    router.replace({ query: next })
  }

  // The FULL public set list (shared, cached with the catalog game view) — the all-sets
  // mode's source, fetched unconditionally so toggling never starts from a spinner.
  const catalogSetsQuery = useSetsQuery(game)
  const catalogSets = computed(() => catalogSetsQuery.data.value?.data ?? [])

  // The active mode's sets, grouped and filterable exactly like the catalog game view:
  // nested sub-sets, instant name/code narrowing, groups kept whole when any member
  // matches (issues #127/#128). One grouping instance over the switched source, so the
  // filter box and header counts track whichever mode is on.
  const sourceSets = computed<CardSet[]>(() =>
    showAllSets.value ? catalogSets.value : heldSets.value,
  )
  const { filter, trimmedFilter, filtering, groups, relatedCount } = useFilteredSetGroups(
    game,
    sourceSets,
  )

  // The active mode's query state, for the loading/error rows.
  const activePending = computed(() =>
    showAllSets.value ? catalogSetsQuery.isPending.value : heldSetsQuery.isPending.value,
  )
  const activeError = computed(() =>
    showAllSets.value ? catalogSetsQuery.isError.value : heldSetsQuery.isError.value,
  )

  // Pinned sets (e.g. Secret Lair) lead as a "Featured" section; the rest break into
  // release-year sections — the same scannable layout as the catalog game view. Used by
  // the all-sets mode only (the held-sets default is a flat newest-first grid).
  const partitioned = computed(() => partitionPinned(groups.value))
  const years = computed(() => groupByYear(partitioned.value.rest))
  const yearLabel = (year: number | null) => (year === null ? 'Unknown year' : String(year))
  const sections = computed(() => {
    const featured = partitioned.value.pinned
    const yearSections = years.value.map((section) => ({
      key: section.year === null ? 'unknown' : String(section.year),
      label: yearLabel(section.year),
      groups: section.groups,
    }))
    return featured.length
      ? [{ key: 'featured', label: 'Featured', groups: featured }, ...yearSections]
      : yearSections
  })

  // Per-set-code held stats each tile shows next to its name: the "N/M owned|wanted"
  // completion count, the "N copies" total, and the preformatted value (null/unpriced sets
  // carry a null the tile omits). Built in one pass and passed to SetGroupGrid as a single
  // `ownership` object; sets the user holds nothing in are simply absent, so in all-sets
  // mode their tiles keep the plain catalog card count. The bulk slice is always computed
  // but only surfaced when `withBulk` is set (the collection): a wish list is a shopping
  // list, so its tiles show only what buying the set's wanted cards would cost.
  const ownership = computed(() => {
    const counts: Record<string, number> = {}
    const copies: Record<string, number> = {}
    const values: Record<string, string | null> = {}
    const bulkValues: Record<string, string | null> = {}
    for (const set of heldSets.value) {
      counts[set.code] = set.owned_cards
      copies[set.code] = set.owned_copies
      values[set.code] = formatUsd(set.owned_value_usd)
      bulkValues[set.code] = formatUsd(set.owned_bulk_value_usd)
    }
    return surface.withBulk ? { counts, copies, values, bulkValues } : { counts, copies, values }
  })
  const totalValue = computed(() => formatUsd(summary.value?.total_value_usd))
  // The bulk (< $1/card) slice of the total value (collection only; the wish list ignores
  // it). Present whenever the total is (both gate on something being priced).
  const bulkValue = computed(() => formatUsd(summary.value?.bulk_value_usd))

  // Stats are worth showing only once something is held.
  const hasStats = computed(() => (summary.value?.unique_cards ?? 0) > 0)

  return {
    game,
    summary,
    heldSets,
    showAllSets,
    setShowAllSets,
    catalogSets,
    sourceSets,
    filter,
    trimmedFilter,
    filtering,
    groups,
    relatedCount,
    activePending,
    activeError,
    sections,
    ownership,
    totalValue,
    bulkValue,
    hasStats,
  }
}
