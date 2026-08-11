import { computed, ref, watch, type Ref } from 'vue'
import { useSetsQuery } from '@/composables/useCatalog'
import type { CardSet } from '@/lib/api'
import { partitionPinnedBy } from '@/lib/setGroups'

/**
 * The set-tile landing engine: filter a list of sets by name/code, pull the pinned ones
 * (Secret Lair) into a leading "Featured" section, and bucket the rest into release-year
 * sections — newest year first, undated sets in a trailing "Unknown year" section.
 *
 * Shared by the two facet-driven landings that aren't the card catalog's own: the sealed
 * products landing (`SealedGameView`) and the preconstructed-deck landing (`PreconsView`).
 * Both start from a facet list that carries only `{ code, name, count }`, so both resolve the
 * icon and the release date the same way — from the public (cached) catalog set list, which
 * the card landing has usually already warmed. A set with no catalog row degrades gracefully:
 * no icon, no date, and it sinks into "Unknown year".
 *
 * The card catalog's own landing does **not** use this: its sets nest related sub-sets
 * (tokens, promos, decks) into groups, which is a different shape entirely (`lib/setGroups`).
 */
export interface SetTileSection<T> {
  /** Stable key: `featured`, a year (`2026`), or `unknown`. */
  key: string
  /** Heading text. */
  label: string
  sets: T[]
}

export function useSetTileSections<T extends { code: string; name?: string | null }>(
  game: Ref<string>,
  sets: Ref<T[]>,
) {
  // Client-side filter box: the whole facet list is already in memory, so narrowing by
  // name/code is instant. Cleared when `game` changes, since these routes reuse the component
  // across `:game` (mirroring useFilteredSetGroups).
  const filter = ref('')
  watch(game, () => {
    filter.value = ''
  })
  const trimmedFilter = computed(() => filter.value.trim())
  const filtering = computed(() => trimmedFilter.value.length > 0)
  const filteredSets = computed(() => {
    const needle = trimmedFilter.value.toLowerCase()
    if (!needle) return sets.value
    return sets.value.filter(
      (set) => set.name?.toLowerCase().includes(needle) || set.code.toLowerCase().includes(needle),
    )
  })

  // The public catalog set list — the same read the card landing uses — resolves each set's
  // code to its catalog row for the tile's icon + release date, and for the year sectioning.
  const catalogSetsQuery = useSetsQuery(game)
  const catalogSetByCode = computed(() => {
    const map: Record<string, CardSet> = {}
    for (const set of catalogSetsQuery.data.value?.data ?? []) map[set.code] = set
    return map
  })
  const releasedAtOf = (set: T) => catalogSetByCode.value[set.code]?.released_at ?? ''

  // Pinned sets (Secret Lair — a continuously-restocked line its 2019 release date would
  // otherwise bury) lead the listing regardless of date. Runs over the *filtered* sets, so a
  // filter that excludes the pinned set drops it from Featured too (mirroring the card
  // landing). A no-op for a game with no pinned set.
  const partitioned = computed(() => partitionPinnedBy(filteredSets.value, (set) => set.code))

  const yearSections = computed<SetTileSection<T>[]>(() => {
    const byYear = new Map<number | null, T[]>()
    for (const set of partitioned.value.rest) {
      const releasedAt = releasedAtOf(set)
      // Slice the leading four digits rather than parsing to a Date — avoids a timezone shift
      // across New Year (matching lib/setGroups.ts's releaseYear).
      const parsed = releasedAt ? Number.parseInt(releasedAt.slice(0, 4), 10) : NaN
      const year = Number.isNaN(parsed) ? null : parsed
      const bucket = byYear.get(year)
      if (bucket) bucket.push(set)
      else byYear.set(year, [set])
    }
    return [...byYear.entries()]
      .map(([year, yearSets]) => ({
        key: year === null ? 'unknown' : String(year),
        label: year === null ? 'Unknown year' : String(year),
        sets: yearSets.sort((a, b) => {
          // Newest release first; then code for a stable order.
          const da = releasedAtOf(a)
          const db = releasedAtOf(b)
          if (da !== db) return da < db ? 1 : -1
          return a.code.localeCompare(b.code)
        }),
      }))
      .sort((a, b) => {
        // Newest year first; undated (null) sinks to the bottom.
        const ya = a.key === 'unknown' ? null : Number(a.key)
        const yb = b.key === 'unknown' ? null : Number(b.key)
        if (ya === yb) return 0
        if (ya === null) return 1
        if (yb === null) return -1
        return yb - ya
      })
  })

  // One flat list to render: the pinned "Featured" section first (when present), then the year
  // sections. Every section is `{ key, label, sets }`, so a template renders both the same way.
  const sections = computed<SetTileSection<T>[]>(() => {
    const featured = partitioned.value.pinned
    if (!featured.length) return yearSections.value
    return [{ key: 'featured', label: 'Featured', sets: featured }, ...yearSections.value]
  })

  return {
    filter,
    trimmedFilter,
    filtering,
    filteredSets,
    catalogSetByCode,
    sections,
    setsPending: catalogSetsQuery.isPending,
  }
}
