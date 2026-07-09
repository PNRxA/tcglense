import { computed, ref, watch, type Ref } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import { useSetsQuery } from '@/composables/useCatalog'
import type { CardSet } from '@/lib/api'
import { filterGroups, findGroup, groupSets, originSetCode, subSetLabel } from '@/lib/setGroups'

/**
 * Related-sub-set grouping + URL scope-navigation for the set view. Reads the
 * (game-keyed, usually-warm) full set list to resolve whether the set on screen
 * belongs to a group of related sub-sets (tokens, promos, decks, …) and drives the
 * "view all together" / "view just one set" scope, keeping the `?related=`/`?from=`
 * scope in the URL so it's shareable and survives a reload.
 *
 * `game`/`code` are the set-view route params (refs). Returns the group derivations
 * the view renders — including the bundled SetScopeBar props and the grouped-view flag
 * (`grouped`) + what it groups by (`groupMode`: Secret Lair drops or derived sub-types) —
 * plus the nav helpers that toggle the scope and the grouped/flat view. `setsPending` is
 * the underlying set-list query's pending flag, which the view gates its flat card fetch on
 * so a cold-loaded grouped/related link doesn't fire a throwaway request.
 *
 * `options.basePath` is the route prefix the scope-nav helpers navigate under
 * (default `/cards`); the collection's set view passes `/collection` so the same
 * grouping derivations drive its own `/collection/:game/sets/:code` pages.
 *
 * `options.preserveQuery` names extra URL query keys the scope-nav must carry across
 * (beyond `q`/`sort`) — e.g. the collection's `ghosts` view mode, which is orthogonal
 * to the include-related scope and must survive toggling it. Empty (the catalog's
 * default) leaves the nav behaviour unchanged.
 */
export function useSetGrouping(
  game: Ref<string>,
  code: Ref<string>,
  options: { basePath?: string; preserveQuery?: string[] } = {},
) {
  const route = useRoute()
  const router = useRouter()
  const basePath = options.basePath ?? '/cards'
  const preserveQuery = options.preserveQuery ?? []

  // The full set list (shared, cached with GameView) tells us whether this set has
  // related sub-sets to fold in.
  const setsQuery = useSetsQuery(game)
  const group = computed(() => findGroup(setsQuery.data.value?.data ?? [], code.value))
  const isMainSet = computed(() => group.value?.main.code === code.value)
  // The count of *other* sets in the group — equal from any member's viewpoint (a
  // child's siblings + the main = the main's children count), so it reads correctly
  // whether you're on the main set or one of its sub-sets.
  const relatedCount = computed(() => group.value?.children.length ?? 0)
  const hasRelated = computed(() => relatedCount.value > 0)
  const setsWord = computed(() => (relatedCount.value === 1 ? 'set' : 'sets'))

  // The "view related" state lives in the URL (?related=1) so it's shareable and
  // survives a reload, but only takes effect when there actually are related sets.
  const includeRelated = computed(() => route.query.related === '1' && hasRelated.value)

  // Every set in the group — the main set first, then its sub-sets — offered in
  // the "view just one set" menu so you can drop into any specific one.
  const members = computed(() => (group.value ? [group.value.main, ...group.value.children] : []))
  const memberOptions = computed(() =>
    members.value.map((member) => ({
      code: member.code,
      name: member.name,
      // The main set keeps its full name for context; sub-sets drop the redundant
      // parent prefix ("Bloomburrow Commander" → "Commander").
      label:
        member.code === group.value?.main.code
          ? member.name
          : subSetLabel(group.value?.main.name ?? '', member.name),
    })),
  )
  // The single set currently on screen, flagged as "current" in the picker (so
  // re-selecting it reads as a no-op rather than a dead click). Null in the grouped
  // view, where no single set is on screen and every option is a real destination.
  const activeSetCode = computed(() => (includeRelated.value ? null : code.value))

  // The set "View just this set" drops back to: the one the grouped view was
  // entered from (?from=…), else the group's main set. This is what fixes landing
  // on the parent set after a sub-set → "view all together" → "view just this set"
  // round-trip — the original set is remembered, not discarded.
  const fromCode = computed(() => (typeof route.query.from === 'string' ? route.query.from : null))
  const originCode = computed(() =>
    group.value ? originSetCode(group.value, fromCode.value) : code.value,
  )
  const originName = computed(
    () => members.value.find((m) => m.code === originCode.value)?.name ?? '',
  )

  // Whether this set is browsable broken down into Secret Lair-style "drops".
  // Sourced from the (game-keyed, usually-warm) set list rather than the per-set
  // metadata, so it's known up front and stays stable across set→set navigation.
  const hasDrops = computed(
    () => setsQuery.data.value?.data.find((s) => s.code === code.value)?.has_drops ?? false,
  )

  // Whether this set has cards with special treatments (borderless, showcase, …) to group
  // by (issue #282). Same set-list source as `hasDrops`, so it's known up front too.
  const hasSubtypes = computed(
    () => setsQuery.data.value?.data.find((s) => s.code === code.value)?.has_subtypes ?? false,
  )

  // A set is browsable under at most one grouping. Drops (Secret Lair) win over derived
  // sub-types — a drop-grouped set's natural breakdown IS its drops — so the two never both
  // apply. `null` = no grouping (only the flat grid).
  const groupMode = computed<'drops' | 'subtypes' | null>(() =>
    hasDrops.value ? 'drops' : hasSubtypes.value ? 'subtypes' : null,
  )

  // The grouped view is the default wherever a grouping exists; ?view=all opts back into the
  // flat grid, and the related-sets view (?related=1) is itself a flat cross-set listing, so
  // it suppresses grouping too. `groupMode` is null for the collection's unscoped '' code, so
  // the all-cards view needs no scope guard of its own.
  const grouped = computed(
    () => groupMode.value !== null && route.query.view !== 'all' && !includeRelated.value,
  )

  // The grouped-view toggle's label — what the set would be grouped BY.
  const groupLabel = computed(() => (groupMode.value === 'subtypes' ? 'By treatment' : 'By drop'))

  // The full prop set SetScopeBar renders, bundled so both set-scoped views bind the
  // bar with one v-bind (the camelCase keys match its props) instead of relaying
  // eight props in lockstep.
  const scopeBarProps = computed(() => ({
    includeRelated: includeRelated.value,
    isMainSet: isMainSet.value,
    mainName: group.value?.main.name ?? '',
    relatedCount: relatedCount.value,
    setsWord: setsWord.value,
    memberOptions: memberOptions.value,
    activeSetCode: activeSetCode.value,
    originName: originName.value,
  }))

  // The passthrough query keys (e.g. the collection's `ghosts` view mode) that ride
  // along every scope-nav so a view preference orthogonal to the scope survives it.
  function preserved(): LocationQueryRaw {
    const out: LocationQueryRaw = {}
    for (const key of preserveQuery) {
      const value = route.query[key]
      if (typeof value === 'string' && value) out[key] = value
    }
    return out
  }

  // Keep the search + sort controls (and any preserved view mode) when only the view
  // scope toggles; paging always restarts (page is intentionally dropped, so it reads
  // back as 1 — switching scope must never strand us on an out-of-range page).
  function listState(): LocationQueryRaw {
    const next: LocationQueryRaw = { ...preserved() }
    if (typeof route.query.q === 'string' && route.query.q) next.q = route.query.q
    if (typeof route.query.sort === 'string' && route.query.sort) next.sort = route.query.sort
    return next
  }

  function setIncludeRelated(on: boolean) {
    if (on) {
      // Root the grouped view at the main set so the URL, heading and counts all
      // agree (matching SetGroup's "View all" link). Entering from a sub-set
      // navigates up to the main set, remembering where we came from (?from=…) so
      // "View just this set" can return there rather than stranding us on the parent;
      // a different set is a fresh scope, so the search/sort don't carry over (but a
      // preserved view mode still does).
      if (group.value && !isMainSet.value) {
        router.replace({
          path: `${basePath}/${game.value}/sets/${group.value.main.code}`,
          query: { ...preserved(), related: '1', from: code.value },
        })
      } else {
        router.replace({ query: { ...listState(), related: '1' } })
      }
    } else {
      viewSingleSet(originCode.value)
    }
  }

  // Leave the grouped view for a single set's own page. Staying on the set already in
  // the route just sheds the related/from scope (search + sort carry over); otherwise
  // route to the chosen set fresh (keeping only a preserved view mode).
  function viewSingleSet(target: string) {
    if (target === code.value) {
      router.replace({ query: listState() })
    } else {
      router.replace({
        path: `${basePath}/${game.value}/sets/${target}`,
        query: { ...preserved() },
      })
    }
  }

  // Toggle the grouped vs flat view of this set. Preserves the search + sort (and any
  // preserved view mode) but sheds the related/from scope and restarts paging (page is
  // dropped by listState) — the two views paginate over different units. ?view=all
  // marks the flat mode; grouped is the bare default.
  function setGroupView(mode: 'grouped' | 'all') {
    const next = listState()
    if (mode === 'all') next.view = 'all'
    router.replace({ query: next })
  }

  return {
    group,
    isMainSet,
    relatedCount,
    hasRelated,
    includeRelated,
    memberOptions,
    activeSetCode,
    originName,
    hasDrops,
    hasSubtypes,
    groupMode,
    grouped,
    groupLabel,
    setsWord,
    scopeBarProps,
    setsPending: setsQuery.isPending,
    setIncludeRelated,
    viewSingleSet,
    setGroupView,
  }
}

/**
 * The client-side filter + nested-grouping pipeline shared by the two set-*landing*
 * views — the catalog game view (`GameView`) and its collection mirror
 * (`GameCollectionView`). Both hold the whole set list in memory, so narrowing by
 * name/code is instant: this owns the filter box state (cleared when `game` changes,
 * since the route reuses the component across `:game`), groups the *whole* list first
 * with {@link groupSets} then filters at the group level with {@link filterGroups} —
 * keeping a group whole when the main set OR any related sub-set matches (issue #128) —
 * and reports the folded-in related-set count over the visible groups.
 *
 * `sets` is the full set list (a ref/computed); accepts the catalog `CardSet` shape or
 * any superset of it (e.g. the collection's `CollectionSet`). The landing view layers
 * its own extras (owned-count maps, pinned/year sectioning) on top of `groups`.
 */
export function useFilteredSetGroups(game: Ref<string>, sets: Ref<CardSet[]>) {
  const filter = ref('')
  watch(game, () => {
    filter.value = ''
  })
  const trimmedFilter = computed(() => filter.value.trim())
  const filtering = computed(() => trimmedFilter.value.length > 0)

  const allGroups = computed(() => groupSets(sets.value))
  const groups = computed(() => filterGroups(allGroups.value, filter.value))
  const relatedCount = computed(() =>
    groups.value.reduce((sum, group) => sum + group.children.length, 0),
  )

  return { filter, trimmedFilter, filtering, groups, relatedCount }
}
