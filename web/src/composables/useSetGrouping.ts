import { computed, type Ref } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import { useSetsQuery } from '@/composables/useCatalog'
import { findGroup, originSetCode, subSetLabel } from '@/lib/setGroups'

/**
 * Related-sub-set grouping + URL scope-navigation for the set view. Reads the
 * (game-keyed, usually-warm) full set list to resolve whether the set on screen
 * belongs to a group of related sub-sets (tokens, promos, decks, …) and drives the
 * "view all together" / "view just one set" scope, keeping the `?related=`/`?from=`
 * scope in the URL so it's shareable and survives a reload.
 *
 * `game`/`code` are the set-view route params (refs). Returns the group derivations
 * the view renders, plus the two nav helpers that toggle the scope. `setsPending`
 * is the underlying set-list query's pending flag, which the view gates its flat
 * card fetch on so a cold-loaded by-drop/related link doesn't fire a throwaway
 * request.
 */
export function useSetGrouping(game: Ref<string>, code: Ref<string>) {
  const route = useRoute()
  const router = useRouter()

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

  // Keep the search + sort controls when only the view scope toggles; paging always
  // restarts (page is intentionally dropped, so it reads back as 1 — switching scope
  // must never strand us on an out-of-range page).
  function listState(): LocationQueryRaw {
    const next: LocationQueryRaw = {}
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
      // a different set is a fresh scope, so the search/sort don't carry over.
      if (group.value && !isMainSet.value) {
        router.replace({
          path: `/cards/${game.value}/sets/${group.value.main.code}`,
          query: { related: '1', from: code.value },
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
  // route to the chosen set fresh.
  function viewSingleSet(target: string) {
    if (target === code.value) {
      router.replace({ query: listState() })
    } else {
      router.replace({ path: `/cards/${game.value}/sets/${target}`, query: {} })
    }
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
    setsPending: setsQuery.isPending,
    // Exposed so the view's own by-drop/flat toggle can preserve the search + sort
    // (and restart paging) the same way the scope nav does.
    listState,
    setIncludeRelated,
    viewSingleSet,
  }
}
