import { computed, ref, toRef, type ComputedRef } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { searchErrorMessage, useCardSearch } from '@/composables/useCardSearch'
import {
  CARD_PAGE_SIZE,
  DROP_PAGE_SIZE,
  SUBTYPE_PAGE_SIZE,
  useAllCardsQuery,
  useGameName,
  useSetCardsQuery,
  useSetDropsQuery,
  useSetQuery,
  useSetSubtypesQuery,
} from '@/composables/useCatalog'
import { useClampPage } from '@/composables/useClampPage'
import { useCurrency } from '@/composables/useCurrency'
import {
  useCollectionDropsQuery,
  useCollectionQuery,
  useCollectionSubtypesQuery,
  useCollectionSummaryQuery,
  useOwnedCounts,
} from '@/composables/useCollection'
import { useSetGrouping } from '@/composables/useSetGrouping'
import { useWishlistCounts } from '@/composables/useWishlist'
import {
  ALL_CARDS_DEFAULT_SORT,
  ALL_CARDS_SORT_OPTIONS,
  COLLECTION_DEFAULT_SORT,
  COLLECTION_SORT_OPTIONS,
  SET_DEFAULT_SORT,
  SET_SORT_OPTIONS,
} from '@/lib/cardSort'
import { type Card, type OwnedCountsMap } from '@/lib/api'
import { formatCompletion, formatCopies, type CountNoun } from '@/lib/ownership'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'
import { useGhostDisplayStore } from '@/stores/ghostDisplay'

// ---- The shared reactive engine behind CollectionBrowseView and WishlistBrowseView ----
//
// The two views are near-identical twins (issue #167): a holding grid for a game, either
// the whole holding (`/{collection,wishlist}/:game/cards`) or scoped to one set
// (`/…/sets/:code`), with three composable view controls layered on — **show-ghosts**
// (#112: also reveal cards not held, dimmed), **by-drop** (#113: group a Secret
// Lair-style set into Scryfall's drops) and **include-related** (#113: fold a set's related
// sub-sets into one listing). They compose into a {held, ghost} × {flat, by-drop} matrix,
// with include-related a flat scope expansion.
//
// The wish list mints a parallel `['wishlist', …]` query-key family over the *same* wire
// shapes as the collection, so the entire engine is type-identical between the two — only
// the query hooks, the completion noun ('owned' vs 'wanted'), the display copy, and the
// wish-list-only "show owned (in collection)" marks differ. Those are the `surface`.

/** The per-surface config the two browse views instantiate the engine with. `useListQuery`
 * etc. are the holding's own query hooks (collection or wish list); the ghost/catalog reads
 * and the collection-counts lookup are shared and imported directly. */
export interface HoldingsBrowseSurface {
  /** Route prefix the scope-nav helpers navigate under — `/collection`, `/wishlist`, or a
   * public collection's `/u/{handle}`. */
  basePath: string
  /** Read-only public surface (a user's public collection): the show-ghosts overlay needs no
   * auth, and the page is indexable (not `noindex`). Defaults false — the authed
   * collection/wishlist surfaces are private + editable, and gate ghosts on being signed in. */
  publicRead?: boolean
  /** The holding's flat / by-drop / by-sub-type / summary query hooks. */
  useListQuery: typeof useCollectionQuery
  useDropsQuery: typeof useCollectionDropsQuery
  useSubtypesQuery: typeof useCollectionSubtypesQuery
  useSummaryQuery: typeof useCollectionSummaryQuery
  /** The holding's browse-badge counts hook (owned counts / wanted counts) for the ghost
   * grid — collection's `useOwnedCounts` or the wish list's `useWishlistCounts`. */
  useCounts: typeof useOwnedCounts
  /** The completion label's noun — undefined ('owned') for the collection, 'wanted' for the
   * wish list. */
  completionNoun?: CountNoun
  /** Wish list only: fetch each rendered card's COLLECTION-owned counts (issue #364
   * follow-up). Feeds the collection-primary quick-add control's count chips (always) and
   * the opt-in "show owned (in collection)" ribbon (issue #213), gated on the setting for
   * display only — the fetch runs regardless. Subsumes the old owned-marks fetch: one
   * `['collection-owned', game, …]` batch shared by both. */
  enableCollectionCounts?: boolean
  /** Fetch each rendered card's wish-list wanted counts into the order-independent
   * `['wishlist-counts', …]` overlay (issue #364 follow-up) to feed the quick-add control's
   * Heart "wanted" chip. On the collection surface it flags cards you *also* wish-list; on the
   * wishlist surface it IS the heart's source — used in place of the entry's list count so a
   * quick-add want edit repaints the heart in place instead of resorting the recency-sorted
   * tiles (the visible list defers its refetch there). Off on surfaces with no heart. */
  enableWishlistHearts?: boolean
  /** Display copy that differs between the two holdings. */
  copy: {
    /** The per-account page title for `usePageMeta`. */
    title: (ctx: { scoped: boolean; setName: string; gameName: string }) => string
    /** The page meta description — public (indexable) surfaces only; omit on the private
     * `noindex` collection/wishlist views. */
    description?: (ctx: { scoped: boolean; setName: string; gameName: string }) => string
    /** The held-only (non-ghost) search-box placeholder. */
    ownSearchPlaceholder: string
    /** The held-only loading-row label. */
    ownLoadingLabel: string
    /** The held-only list-error message. */
    ownErrorMessage: string
  }
}

/**
 * The whole shared script engine for a holding browse view. Pass the view's `{ game, code }`
 * props and its `surface` config; the returned refs/computeds are what the (per-holding)
 * template binds. The two templates legitimately differ — the collection renders a bulk-value
 * slice and no `list`/collection-counts props; the wish list omits bulk value and threads
 * `list="wishlist"` + `:collection-counts`/`:owned-marks` through the grids — so only the
 * script is shared.
 */
export function useHoldingsBrowse(
  props: { game: string; code?: string },
  surface: HoldingsBrowseSurface,
) {
  const game = toRef(props, 'game')
  const money = useCurrency()
  const code = toRef(props, 'code')
  const setCode = computed(() => props.code || undefined)
  const scoped = computed(() => !!setCode.value)
  // A stable string code for the grouping/drops helpers, which expect a plain ref (the
  // unscoped view passes '', so they resolve to no group / no drops and stay inert).
  const groupCode = computed(() => props.code ?? '')

  const route = useRoute()
  const router = useRouter()
  const gameName = useGameName(game)
  const auth = useAuthStore()
  // Whether the show-ghosts overlay (the full catalog with held cards marked + the rest
  // dimmed) may fetch. On the authed collection/wishlist it's a signed-in feature (its counts
  // come from the user's own holdings); on a read-only public collection it's always
  // available — the counts are the *owner's*, read token-lessly by handle.
  const canGhost = computed(() => surface.publicRead === true || auth.isAuthenticated)

  // Show-ghosts mode (issue #112): when on (`?ghosts=1`), the grid also shows the cards in
  // scope the user *doesn't* hold — dimmed "ghosts" — so the gaps read at a glance and can
  // be quick-added in place. Defaults off. It composes with by-drop and include-related.
  const showGhosts = computed(() => route.query.ghosts === '1')

  function setShowGhosts(on: boolean) {
    const next = { ...route.query }
    if (on) next.ghosts = '1'
    else delete next.ghosts
    // The two modes list different cards and sort differently, so a page number and a
    // mode-specific sort don't carry across the toggle — drop both so the target mode
    // starts on page 1 at its own default order (held = recency; ghosts = catalog order).
    // The by-drop / include-related scope (view / related / from) is preserved.
    delete next.page
    delete next.sort
    router.replace({ query: next })
  }

  // Related-sub-set grouping + the "view all together" scope nav + the by-drop view, all
  // keyed off the (game-cached) public set list — reused from the catalog set view, but
  // pointed at the holding's own routes. The unscoped all-cards view passes code '', so
  // `hasRelated`/`hasDrops`/`byDrop` resolve false and the scope controls stay inert without
  // a scoped guard here. By-drop composes with show-ghosts.
  const {
    group,
    relatedCount,
    hasRelated,
    includeRelated,
    groupMode,
    grouped,
    groupLabel,
    setsWord,
    scopeBarProps,
    setsPending,
    setIncludeRelated,
    viewSingleSet,
    setGroupView,
  } = useSetGrouping(game, groupCode, { basePath: surface.basePath, preserveQuery: ['ghosts'] })

  // A grouped set breaks down into either Secret Lair drops or card sub-types (never both —
  // see `groupMode`); split the flag so each mode's query can be selected.
  const byDrop = computed(() => grouped.value && groupMode.value === 'drops')
  const bySubtype = computed(() => grouped.value && groupMode.value === 'subtypes')

  // A set's display name for the header/breadcrumb (public, cached). Only fetched for the
  // set-scoped view; falls back to the upper-cased code until it loads or if it's unknown.
  const setQuery = useSetQuery(game, groupCode, scoped)
  const setName = computed(() =>
    scoped.value ? (setQuery.data.value?.name ?? code.value?.toUpperCase() ?? '') : '',
  )
  // When folding in related sets, the heading is rooted at the group's main set.
  const heading = computed(() => {
    if (!scoped.value) return 'All cards'
    return includeRelated.value && group.value ? group.value.main.name : setName.value
  })

  // A private per-account page (collection/wishlist) is kept out of search indexes; a
  // public collection is indexable and carries a description.
  const metaDescription = surface.copy.description
  usePageMeta({
    title: () =>
      surface.copy.title({
        scoped: scoped.value,
        setName: setName.value,
        gameName: gameName.value,
      }),
    description: metaDescription
      ? () =>
          metaDescription({
            scoped: scoped.value,
            setName: setName.value,
            gameName: gameName.value,
          })
      : undefined,
    canonicalPath: () =>
      scoped.value
        ? `${surface.basePath}/${game.value}/sets/${code.value}`
        : `${surface.basePath}/${game.value}/cards`,
    noindex: !surface.publicRead,
  })

  // In show-ghosts mode the flat grid is really the catalog list (held + not), so it offers
  // the catalog's sorts — a set's collector order, or the all-cards name order — while the
  // held-only mode keeps the holding's recency-first sorts. Recency is meaningless for cards
  // you don't hold, so the two sort sets (and their defaults) swap with the mode; the getters
  // let `useCardSearch` re-clamp the committed sort when the toggle flips. (By-drop hides the
  // sort menu — a fixed drop order — so its sort set is moot there.)
  const sortOptions = computed(() =>
    showGhosts.value
      ? scoped.value
        ? SET_SORT_OPTIONS
        : ALL_CARDS_SORT_OPTIONS
      : COLLECTION_SORT_OPTIONS,
  )
  const defaultSort = computed(() =>
    showGhosts.value
      ? scoped.value
        ? SET_DEFAULT_SORT
        : ALL_CARDS_DEFAULT_SORT
      : COLLECTION_DEFAULT_SORT,
  )
  const validSorts = computed(() => sortOptions.value.map((option) => option.value))

  // Page, search and sort live in the URL query (like the catalog browse views), so they
  // survive opening a card and pressing Back and are shareable/reload-safe.
  const { page, searchInput, query, sort } = useCardSearch(defaultSort, validSorts)

  // A cold scoped link must wait for the set list (which decides byDrop/hasDrops) before
  // firing a flat fetch, so a drop-set link doesn't flash the flat grid then discard it. The
  // unscoped view has no drops/related, so it never waits.
  const flatReady = computed(() => !scoped.value || !setsPending.value)

  // ---- Data sources: {held, ghost} × {flat, grouped}. Exactly one is enabled. The grouped
  // column is itself either by-drop or by-sub-type (never both — `groupMode`), so each has
  // both query hooks; only the one matching this set's mode ever fetches. ----

  // Held + flat (the default). Idle when ghosts or a grouped view is active.
  const listQuery = surface.useListQuery(game, page, query, sort, setCode, {
    includeRelated,
    enabled: computed(() => !showGhosts.value && !grouped.value && flatReady.value),
  })
  const entries = computed(() => listQuery.data.value?.data ?? [])

  // Held + grouped: the user's held cards grouped into Secret Lair drops or card sub-types.
  const heldDropsQuery = surface.useDropsQuery(game, groupCode, page, query, {
    enabled: computed(() => !showGhosts.value && byDrop.value),
  })
  const heldSubtypesQuery = surface.useSubtypesQuery(game, groupCode, page, query, {
    enabled: computed(() => !showGhosts.value && bySubtype.value),
  })
  const heldGroupsQuery = computed(() => (bySubtype.value ? heldSubtypesQuery : heldDropsQuery))
  const groups = computed(() => heldGroupsQuery.value.data.value?.data ?? [])

  // Ghost + flat: the public catalog list for this scope (held + not), paginated + searchable
  // + sortable exactly like the catalog browse grids, spanning the set's group when
  // include-related is on. Reuses the catalog views' query hooks (and so their cache entries).
  // Signed-in + show-ghosts + flat only (and, when scoped, once the set list has settled). The
  // whole grid lives behind the signed-in template, so a signed-out visitor landing on
  // `?ghosts=1` sees the sign-in prompt — don't fetch for them.
  const ghostFlat = computed(
    () => canGhost.value && showGhosts.value && !grouped.value && flatReady.value,
  )
  // Both hooks are called unconditionally — this component backs both the all-cards and
  // set-scoped routes, so `scoped` can flip on the same reused instance — with `enabled`
  // gates picking the one matching the current scope.
  const ghostSetCardsQuery = useSetCardsQuery(game, groupCode, {
    page,
    query,
    sort,
    defaultSort: SET_DEFAULT_SORT,
    includeRelated,
    enabled: computed(() => ghostFlat.value && scoped.value),
  })
  const ghostAllCardsQuery = useAllCardsQuery(game, {
    page,
    query,
    sort,
    defaultSort: ALL_CARDS_DEFAULT_SORT,
    enabled: computed(() => ghostFlat.value && !scoped.value),
  })
  const ghostQuery = computed(() => (scoped.value ? ghostSetCardsQuery : ghostAllCardsQuery))
  const ghostCards = computed<Card[]>(() => ghostQuery.value.data.value?.data ?? [])
  // The ghost list is showing a genuine, current result (not the previous page held by
  // keepPreviousData) — used to gate the completion label so it isn't computed from a stale
  // total while a filter/page change reloads.
  const ghostSettled = computed(
    () => ghostQuery.value.isSuccess.value && !ghostQuery.value.isPlaceholderData.value,
  )

  // Ghost + grouped: the catalog's by-drop / by-sub-type endpoint (every card in each group),
  // so the groups show what you're missing (dimmed) alongside what you hold.
  const ghostDropsQuery = useSetDropsQuery(game, groupCode, {
    page,
    query,
    enabled: computed(() => canGhost.value && showGhosts.value && byDrop.value),
  })
  const ghostSubtypesQuery = useSetSubtypesQuery(game, groupCode, {
    page,
    query,
    enabled: computed(() => canGhost.value && showGhosts.value && bySubtype.value),
  })
  const ghostGroupsQuery = computed(() => (bySubtype.value ? ghostSubtypesQuery : ghostDropsQuery))
  const ghostGroups = computed(() => ghostGroupsQuery.value.data.value?.data ?? [])

  // Held counts for the visible ghost cards (the flat page, or every group's cards): they
  // drive both the count badges and which cards render as ghosts (a card absent from the map
  // is dimmed). `ownershipReady` gates the dimming so held cards don't flash as ghosts before
  // their counts load. Empty/idle in the held-only modes.
  const ghostVisibleCards = computed<Card[]>(() =>
    grouped.value ? ghostGroups.value.flatMap((g) => g.cards) : ghostCards.value,
  )
  const { ownership, ready: ownershipReady } = surface.useCounts(game, ghostVisibleCards)

  // The cards the active mode currently renders (any of {held,ghost}×{flat,grouped}) — the
  // lookup scope for the secondary per-card overlays (owned marks + wish-list hearts).
  const renderedCards = computed<Card[]>(() => {
    if (showGhosts.value) return ghostVisibleCards.value
    return grouped.value
      ? groups.value.flatMap((g) => g.cards.map((entry) => entry.card))
      : entries.value.map((entry) => entry.card)
  })

  // Collection-owned counts on the wish-list browse surface (issue #364 follow-up). The
  // quick-add control is collection-primary everywhere, so each rendered card's COLLECTION
  // counts must always be known to show its Layers/Sparkles chips — fetched *unconditionally*
  // over whatever cards the active mode renders (any of the four {list,ghost}×{flat,by-drop}
  // shapes), one `['collection-owned', game, …]` batch, identical cost to the collection page.
  // `collectionCounts` feeds the control's primary chips; `ownedMarks` reuses the very same map
  // for the opt-in "show owned (in collection)" ribbon (issue #213), gated on the persisted
  // setting for display only — the fetch runs regardless (vue-query dedupes, and the always-on
  // observer keeps it enabled). Distinct data from the wish-list counts above (`ownership`,
  // which on this surface means wish-list membership). Collection views leave this off
  // (`enableCollectionCounts` unset), so nothing extra is fetched.
  let collectionCounts: ComputedRef<OwnedCountsMap | undefined> = computed(() => undefined)
  let ownedMarks: ComputedRef<OwnedCountsMap | undefined> = computed(() => undefined)
  if (surface.enableCollectionCounts) {
    const { ownership: collectionOwnership } = useOwnedCounts(game, renderedCards)
    collectionCounts = computed(() => collectionOwnership.value)
    const ghostDisplay = useGhostDisplayStore()
    const showOwnedMarks = computed(() => auth.isAuthenticated && ghostDisplay.showOwned)
    ownedMarks = computed(() => (showOwnedMarks.value ? collectionOwnership.value : undefined))
  }

  // Wish-list hearts (issue #364 follow-up): fetch each rendered card's wanted counts into the
  // order-independent `['wishlist-counts', …]` overlay (auth-gated, empty while signed out)
  // over the same rendered cards. On the collection surface it flags cards you also wish-list;
  // on the wishlist surface it IS the heart's source (threaded to the grids as `:wishlist`),
  // replacing the reordering list so a want edit repaints the heart in place. This write's
  // `['wishlist-counts', …]` invalidation refetches it even while the list refetch is deferred.
  let wishlistCounts: ComputedRef<OwnedCountsMap | undefined> = computed(() => undefined)
  if (surface.enableWishlistHearts) {
    const wc = useWishlistCounts(game, renderedCards)
    wishlistCounts = computed(() => wc.ownership.value)
  }

  // The held stats for the current scope (all cards / a set / a set + its related group,
  // tracking `includeRelated`), unfiltered by the search box. Fetched in every mode: it drives
  // the scoped **value** shown next to the count and the scope's "X/Y {owned,wanted}"
  // completion count (the show-ghosts view reads it as held-of-catalog; the held-only view
  // reads held-of-`scopeTotal`). Because it spans the group under include-related, both read
  // correctly there too. (Reuses the landing's cache key, so arriving from the landing is a
  // cache hit for the all-cards scope.)
  const summaryQuery = surface.useSummaryQuery(game, setCode, { includeRelated })
  const heldUnique = computed(() => summaryQuery.data.value?.unique_cards ?? 0)

  // The catalog total of cards in the current scope — the denominator for the set-scoped
  // held-only completion count. A single set uses its own card_count; include-related sums the
  // whole group (root + related sub-sets), mirroring what the ghost list counts. Null off a set
  // scope (the whole-game "all cards" view has no meaningful completion target) or before the
  // set metadata / set list has loaded.
  const scopeTotal = computed<number | null>(() => {
    if (!scoped.value) return null
    if (includeRelated.value) {
      const g = group.value
      if (!g) return null
      return [g.main, ...g.children].reduce((sum, s) => sum + s.card_count, 0)
    }
    return setQuery.data.value?.card_count ?? null
  })
  // The scope's held value, split into the total and its bulk (< $1/card) slice, both formatted
  // (null while loading or when nothing in scope is priced). The wish-list template never
  // renders the bulk slice (a shopping list only cares about cost), but computing it here is
  // harmless. Shown only when there's no active search — the values are the whole scope's, so
  // pairing them with a search-filtered count would misread.
  const scopeTotalValue = computed(() =>
    query.value ? null : money.formatUsd(summaryQuery.data.value?.total_value_usd),
  )
  const scopeBulkValue = computed(() =>
    query.value ? null : money.formatUsd(summaryQuery.data.value?.bulk_value_usd),
  )
  // The scope's total held copies (with duplicates) as "N copies", shown next to the count when
  // there are more copies than distinct cards. Like the value, it's the whole scope's figure,
  // so it's hidden while a search filters the list.
  const scopeCopiesLabel = computed(() => {
    if (query.value) return null
    const s = summaryQuery.data.value
    return s && s.total_cards > s.unique_cards ? formatCopies(s.total_cards) : null
  })

  // ---- Active data source: exactly one of the {held,ghost}×{flat,grouped} queries is enabled
  // at a time. Pick it once — by reference, so its reactive fields stay live — and derive the
  // list state off it, instead of re-branching on the mode in every computed. The grouped
  // queries themselves switch on `groupMode` (drops vs sub-types). ----
  const active = computed(() =>
    showGhosts.value
      ? grouped.value
        ? ghostGroupsQuery.value
        : ghostQuery.value
      : grouped.value
        ? heldGroupsQuery.value
        : listQuery,
  )
  const total = computed(() => active.value.data.value?.total ?? 0)
  const listPending = computed(() => active.value.isPending.value)
  const listIsError = computed(() => active.value.isError.value)
  const listError = computed(() => active.value.error.value)
  const listIsFetching = computed(() => active.value.isFetching.value)
  const listIsSuccess = computed(() => active.value.isSuccess.value)
  // Refetching over stale results (page/filter change held by keepPreviousData): drives an
  // honest "Updating…" cue on the count line rather than silently showing the old total.
  const updating = computed(() => listIsFetching.value && active.value.isPlaceholderData.value)
  // Derive from the active page's own `data` array (every page shape exposes one), not from
  // `total`, so a previous page held by keepPreviousData still reads as "has cards" while the
  // next loads.
  const hasCards = computed(() => (active.value.data.value?.data?.length ?? 0) > 0)
  // A malformed search query comes back as 422; surface its message inline.
  const searchError = computed(() => searchErrorMessage(listError.value))

  // The active view sets the pagination unit: groups (grouped) or printings (flat). Drops and
  // sub-types share a page size, but pick by mode so the two can diverge later.
  const groupPageSize = computed(() => (bySubtype.value ? SUBTYPE_PAGE_SIZE : DROP_PAGE_SIZE))
  const pageSize = computed(() => (grouped.value ? groupPageSize.value : CARD_PAGE_SIZE))
  // The top of the results (the controls row above the grid) — both pagers scroll here so a
  // page change starts at the top of the listing, clearing the sticky search bar (issue #258).
  const resultsTop = ref<HTMLElement | null>(null)
  useClampPage(page, () => ({
    ready: listIsSuccess.value,
    total: total.value,
    pageSize: pageSize.value,
  }))

  const countLabel = computed(() => {
    const n = total.value
    // A grouped view counts its groups (drops or sub-types).
    if (grouped.value) {
      const unit = bySubtype.value ? 'sub-type' : 'drop'
      const label = `${n.toLocaleString()} ${n === 1 ? unit : `${unit}s`}`
      return query.value ? `${label} matching “${query.value}”` : label
    }
    const word = n === 1 ? 'card' : 'cards'
    if (query.value) return `${n.toLocaleString()} ${word} matching “${query.value}”`
    // Show-ghosts (flat) leads with completion (held ⊆ scope): `n` is the catalog total in
    // scope, `heldUnique` how many you hold. Only once both the (unfiltered) summary and the
    // ghost list have genuinely settled, and there's something in scope — otherwise a mid-load
    // `total` of 0 (or a stale filtered total) would misread. The summary spans the same
    // set/group the ghost list does (it tracks include-related), so this reads right there too.
    if (showGhosts.value) {
      if (summaryQuery.isSuccess.value && ghostSettled.value && n > 0) {
        return formatCompletion(heldUnique.value, n, surface.completionNoun)
      }
      return `${n.toLocaleString()} ${word}`
    }
    // Held-only, set-scoped: the same "X/Y {owned,wanted}" completion, but `n` is now the held
    // count and `scopeTotal` the set/group's catalog total — so the browse header matches the
    // landing tiles. Gated on the held list settling so it doesn't flash "0/Y" while loading;
    // the whole-game "all cards" view has no scope total, so it keeps the plain "N cards".
    if (scoped.value && listIsSuccess.value && scopeTotal.value != null && scopeTotal.value > 0) {
      return formatCompletion(n, scopeTotal.value, surface.completionNoun)
    }
    return `${n.toLocaleString()} ${word}`
  })

  const searchPlaceholder = computed(() => {
    if (scoped.value) return 'Search this set — name, c:r, t:goblin…'
    return showGhosts.value
      ? 'Search all cards — name, c:r, t:goblin…'
      : surface.copy.ownSearchPlaceholder
  })

  // The ghost data source is the public catalog, so its loading/error copy is neutral
  // ("cards"); the held-only mode keeps the holding-worded copy.
  const loadingLabel = computed(() =>
    showGhosts.value ? 'Loading cards…' : surface.copy.ownLoadingLabel,
  )
  const errorMessage = computed(() =>
    showGhosts.value ? "Couldn't load cards. Please retry." : surface.copy.ownErrorMessage,
  )

  return {
    game,
    code,
    setCode,
    scoped,
    groupCode,
    gameName,
    showGhosts,
    setShowGhosts,
    group,
    relatedCount,
    hasRelated,
    includeRelated,
    groupMode,
    grouped,
    groupLabel,
    setsWord,
    scopeBarProps,
    setsPending,
    setIncludeRelated,
    viewSingleSet,
    setGroupView,
    byDrop,
    bySubtype,
    setName,
    heading,
    page,
    searchInput,
    query,
    sort,
    sortOptions,
    entries,
    groups,
    ghostCards,
    ghostGroups,
    ownership,
    ownershipReady,
    ownedMarks,
    collectionCounts,
    wishlistCounts,
    heldUnique,
    scopeTotal,
    scopeTotalValue,
    scopeBulkValue,
    scopeCopiesLabel,
    total,
    listPending,
    listIsError,
    searchError,
    updating,
    hasCards,
    listIsFetching,
    groupPageSize,
    resultsTop,
    countLabel,
    searchPlaceholder,
    loadingLabel,
    errorMessage,
  }
}
