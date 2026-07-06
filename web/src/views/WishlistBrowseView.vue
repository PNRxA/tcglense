<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import { buttonVariants } from '@/components/ui/button'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardGridSkeleton from '@/components/cards/CardGridSkeleton.vue'
import GhostToggle from '@/components/cards/GhostToggle.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import AdvancedSearchPanel from '@/components/cards/AdvancedSearchPanel.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import CollectionGrid from '@/components/collection/CollectionGrid.vue'
import DropSection from '@/components/cards/DropSection.vue'
import DropViewToggle from '@/components/cards/DropViewToggle.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import SearchSyntaxHint from '@/components/cards/SearchSyntaxHint.vue'
import SetScopeBar from '@/components/cards/SetScopeBar.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import CollectionSignInPrompt from '@/components/collection/CollectionSignInPrompt.vue'
import { searchErrorMessage, useCardSearch } from '@/composables/useCardSearch'
import {
  CARD_PAGE_SIZE,
  DROP_PAGE_SIZE,
  useAllCardsQuery,
  useGameName,
  useSetCardsQuery,
  useSetDropsQuery,
  useSetQuery,
} from '@/composables/useCatalog'
import { useClampPage } from '@/composables/useClampPage'
import {
  useWishlistCounts,
  useWishlistDropsQuery,
  useWishlistQuery,
  useWishlistSummaryQuery,
} from '@/composables/useWishlist'
import { useOwnedCounts } from '@/composables/useCollection'
import { useSetGrouping } from '@/composables/useSetGrouping'
import {
  ALL_CARDS_DEFAULT_SORT,
  ALL_CARDS_SORT_OPTIONS,
  COLLECTION_DEFAULT_SORT,
  COLLECTION_SORT_OPTIONS,
  SET_DEFAULT_SORT,
  SET_SORT_OPTIONS,
} from '@/lib/cardSort'
import { type Card } from '@/lib/api'
import { formatUsd } from '@/lib/money'
import { formatCompletion, formatCopies } from '@/lib/ownership'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'
import { useGhostDisplayStore } from '@/stores/ghostDisplay'

// Wishlisted cards for a game, either the whole wish list (`/wishlist/:game/cards`) or
// scoped to one set (`/wishlist/:game/sets/:code`) — CollectionBrowseView's twin (issue
// #167), with "ownership" meaning wish-list membership throughout: the badges show
// wanted counts, and a ghost is a card *not on the list*. The two routes share this
// view; `code` is the only difference (undefined = all cards).
//
// The same three view controls compose on top ({list, ghost} × {flat, by-drop}, with
// include-related a flat scope expansion). Show-ghosts defaults OFF like the
// collection (`?ghosts=1` opts in): the bare URL lists just your wishlisted cards,
// and the toggle swaps in the whole catalog in scope with non-wishlisted cards dimmed
// — the add-in-place browse mode, whose count controls write to the wish list.
const props = defineProps<{ game: string; code?: string }>()
const game = toRef(props, 'game')
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

// Show-ghosts mode, default OFF like the collection view: the bare URL lists just the
// wishlisted cards; `?ghosts=1` swaps in the whole catalog in scope with
// non-wishlisted cards dimmed. Composes with by-drop and include-related.
const showGhosts = computed(() => route.query.ghosts === '1')

function setShowGhosts(on: boolean) {
  const next = { ...route.query }
  // Off is the default, so it's the bare URL; on is the explicit `?ghosts=1`.
  if (on) next.ghosts = '1'
  else delete next.ghosts
  // The two modes list different cards and sort differently, so a page number and a
  // mode-specific sort don't carry across the toggle — drop both so the target mode
  // starts on page 1 at its own default order (list = recency; ghosts = catalog order).
  // The by-drop / include-related scope (view / related / from) is preserved.
  delete next.page
  delete next.sort
  router.replace({ query: next })
}

// Related-sub-set grouping + the "view all together" scope nav + the by-drop view, all
// keyed off the (game-cached) public set list — reused from the catalog set view, but
// pointed at the wish list's own routes. The unscoped all-cards view passes code '',
// so `hasRelated`/`hasDrops`/`byDrop` resolve false and the scope controls stay inert
// without a scoped guard here. By-drop composes with show-ghosts (wishlisted drops vs.
// the catalog's every-card drops).
const {
  group,
  relatedCount,
  hasRelated,
  includeRelated,
  hasDrops,
  byDrop,
  setsWord,
  scopeBarProps,
  setsPending,
  setIncludeRelated,
  viewSingleSet,
  setDropView,
} = useSetGrouping(game, groupCode, { basePath: '/wishlist', preserveQuery: ['ghosts'] })

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

// Per-account page — kept out of search indexes.
usePageMeta({
  title: () =>
    scoped.value
      ? `${setName.value} — your ${gameName.value} wish list`
      : `Your ${gameName.value} wish list cards`,
  canonicalPath: () =>
    scoped.value ? `/wishlist/${game.value}/sets/${code.value}` : `/wishlist/${game.value}/cards`,
  noindex: true,
})

// In show-ghosts mode the flat grid is really the catalog list (wanted + not), so it
// offers the catalog's sorts — a set's collector order, or the all-cards name order —
// while the list-only mode keeps the wish list's recency-first sorts. Recency is
// meaningless for cards not on the list, so the two sort sets (and their defaults) swap
// with the mode; the getters let `useCardSearch` re-clamp the committed sort when the
// toggle flips. (By-drop hides the sort menu — a fixed drop order — so its sort set is
// moot there.)
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

// ---- Four data sources: {list, ghost} × {flat, by-drop}. Exactly one is enabled. ----

// Wishlisted + flat. Idle when ghosts (the default here) or by-drop is active.
const wishlistQuery = useWishlistQuery(game, page, query, sort, setCode, {
  includeRelated,
  enabled: computed(() => !showGhosts.value && !byDrop.value && flatReady.value),
})
const listedEntries = computed(() => wishlistQuery.data.value?.data ?? [])

// Wishlisted + by drop: the user's wishlisted cards grouped into Secret Lair drops.
const listedDropsQuery = useWishlistDropsQuery(game, groupCode, page, query, {
  enabled: computed(() => !showGhosts.value && byDrop.value),
})
const listedDropGroups = computed(() => listedDropsQuery.data.value?.data ?? [])

// Ghost + flat: the public catalog list for this scope (wanted + not), paginated +
// searchable + sortable exactly like the catalog browse grids, spanning the set's group
// when include-related is on. Reuses the catalog views' query hooks (and so their cache
// entries — a just-browsed set is a cache hit).
// Signed-in + show-ghosts + flat only (and, when scoped, once the set list has settled).
// The whole grid lives behind the signed-in template, so a signed-out visitor sees the
// sign-in prompt — don't fetch for them.
const ghostFlat = computed(
  () => auth.isAuthenticated && showGhosts.value && !byDrop.value && flatReady.value,
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

// Ghost + by drop: the catalog's by-drop endpoint (every card in each drop), so the drops
// show what you haven't wished for (dimmed) alongside what you have.
const ghostDropsQuery = useSetDropsQuery(game, groupCode, {
  page,
  query,
  enabled: computed(() => auth.isAuthenticated && showGhosts.value && byDrop.value),
})
const ghostDropGroups = computed(() => ghostDropsQuery.data.value?.data ?? [])

// Wanted counts for the visible ghost cards (the flat page, or every drop's cards): they
// drive both the count badges and which cards render as ghosts (a card absent from the
// map — i.e. not on the wish list — is dimmed). `ownershipReady` gates the dimming so
// wishlisted cards don't flash as ghosts before their counts load. Empty/idle in the
// list-only modes.
const ghostVisibleCards = computed<Card[]>(() =>
  byDrop.value ? ghostDropGroups.value.flatMap((drop) => drop.cards) : ghostCards.value,
)
const { ownership, ready: ownershipReady } = useWishlistCounts(game, ghostVisibleCards)

// "Show owned (in collection)" (issue #213): a persisted setting on the ghost button's
// dropdown that flags which cards on the page you already own in your *collection* while you
// shop the wish list. Distinct data from the wish-list counts above (`ownership`) — it's the
// collection's holdings — so it needs its own lookup, over whatever cards the active mode
// renders (any of the four {list,ghost}×{flat,by-drop} shapes). Fetched only when the setting
// is on and signed in; the marks pass to the grids as `ownedMarks` (undefined = no markers).
const ghostDisplay = useGhostDisplayStore()
const showOwnedMarks = computed(() => auth.isAuthenticated && ghostDisplay.showOwned)
const markableCards = computed<Card[]>(() => {
  // Ghost mode renders the same cards `ghostVisibleCards` already derives; the listed-only
  // modes render the wishlisted holdings' cards.
  if (showGhosts.value) return ghostVisibleCards.value
  return byDrop.value
    ? listedDropGroups.value.flatMap((drop) => drop.cards.map((entry) => entry.card))
    : listedEntries.value.map((entry) => entry.card)
})
const { ownership: collectionOwnership } = useOwnedCounts(game, markableCards, {
  enabled: showOwnedMarks,
})
const ownedMarks = computed(() => (showOwnedMarks.value ? collectionOwnership.value : undefined))

// The wish-list stats for the current scope (all cards / a set / a set + its related
// group, tracking `includeRelated`), unfiltered by the search box. Fetched in every mode:
// it drives the scoped wish-list **value** shown next to the count and the scope's
// "X/Y wanted" completion count (the show-ghosts view reads it as wanted-of-catalog; the
// list-only view reads wanted-of-`scopeTotal`). Because it spans the group under
// include-related, both read correctly there too. (Reuses the landing's cache key, so
// arriving from `/wishlist/:game` is a cache hit for the all-cards scope.)
const summaryQuery = useWishlistSummaryQuery(game, setCode, { includeRelated })
const wantedUnique = computed(() => summaryQuery.data.value?.unique_cards ?? 0)

// The catalog total of cards in the current scope — the denominator for the set-scoped
// list-only "X/Y wanted" completion count. A single set uses its own card_count;
// include-related sums the whole group (root + related sub-sets), mirroring what the
// ghost list counts. Null off a set scope (the whole-game "all cards" view has no
// meaningful completion target) or before the set metadata / set list has loaded.
const scopeTotal = computed<number | null>(() => {
  if (!scoped.value) return null
  if (includeRelated.value) {
    const g = group.value
    if (!g) return null
    return [g.main, ...g.children].reduce((sum, s) => sum + s.card_count, 0)
  }
  return setQuery.data.value?.card_count ?? null
})
// What the scope's wishlisted cards would cost, formatted (null while loading or when
// nothing in scope is priced). No bulk slice on wish-list pages — a wish list is a
// shopping list, so only what it costs matters. Shown only when there's no active
// search — the value is the whole scope's, so pairing it with a search-filtered count
// would misread.
const scopeTotalValue = computed(() =>
  query.value ? null : formatUsd(summaryQuery.data.value?.total_value_usd),
)
// The scope's total wanted copies (with duplicates) as "N copies", shown next to the
// count when more copies than distinct cards are wanted. Like the value, it's the whole
// scope's figure, so it's hidden while a search filters the list.
const scopeCopiesLabel = computed(() => {
  if (query.value) return null
  const s = summaryQuery.data.value
  return s && s.total_cards > s.unique_cards ? formatCopies(s.total_cards) : null
})

// ---- Active data source: exactly one of the four {list,ghost}×{flat,by-drop} queries is
// enabled at a time. Pick it once — by reference, so its reactive fields stay live — and
// derive the list state off it, instead of re-branching on the mode in every computed. ----
const active = computed(() =>
  showGhosts.value
    ? byDrop.value
      ? ghostDropsQuery
      : ghostQuery.value
    : byDrop.value
      ? listedDropsQuery
      : wishlistQuery,
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
// next loads — matching the collection browse view.
const hasCards = computed(() => (active.value.data.value?.data?.length ?? 0) > 0)
// A malformed search query comes back as 422; surface its message inline.
const searchError = computed(() => searchErrorMessage(listError.value))

// The active view sets the pagination unit: drops (by-drop) or printings (flat).
const pageSize = computed(() => (byDrop.value ? DROP_PAGE_SIZE : CARD_PAGE_SIZE))
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
  // By-drop counts drops.
  if (byDrop.value) {
    const label = `${n.toLocaleString()} ${n === 1 ? 'drop' : 'drops'}`
    return query.value ? `${label} matching “${query.value}”` : label
  }
  const word = n === 1 ? 'card' : 'cards'
  if (query.value) return `${n.toLocaleString()} ${word} matching “${query.value}”`
  // Show-ghosts (flat) leads with completion (wanted ⊆ scope): `n` is the catalog total in
  // scope, `wantedUnique` how many are wishlisted. Only once both the (unfiltered) summary
  // and the ghost list have genuinely settled, and there's something in scope — otherwise a
  // mid-load `total` of 0 (or a stale filtered total) would misread. The summary spans the
  // same set/group the ghost list does (it tracks include-related), so this reads right
  // there too.
  if (showGhosts.value) {
    if (summaryQuery.isSuccess.value && ghostSettled.value && n > 0) {
      return formatCompletion(wantedUnique.value, n, 'wanted')
    }
    return `${n.toLocaleString()} ${word}`
  }
  // List-only, set-scoped: the same "X/Y wanted" completion, but `n` is now the wishlisted
  // count and `scopeTotal` the set/group's catalog total — so the browse header matches the
  // landing tiles. Gated on the list settling so it doesn't flash "0/Y" while loading; the
  // whole-game "all cards" view has no scope total, so it keeps the plain "N cards".
  if (scoped.value && listIsSuccess.value && scopeTotal.value != null && scopeTotal.value > 0) {
    return formatCompletion(n, scopeTotal.value, 'wanted')
  }
  return `${n.toLocaleString()} ${word}`
})

const searchPlaceholder = computed(() => {
  if (scoped.value) return 'Search this set — name, c:r, t:goblin…'
  return showGhosts.value
    ? 'Search all cards — name, c:r, t:goblin…'
    : 'Search your wish list — name, c:r, t:goblin…'
})

// The ghost data source is the public catalog, so its loading/error copy is neutral
// ("cards"); the list-only mode keeps the wish-list-worded copy.
const loadingLabel = computed(() =>
  showGhosts.value ? 'Loading cards…' : 'Loading your wish list…',
)
const errorMessage = computed(() =>
  showGhosts.value
    ? "Couldn't load cards. Please retry."
    : "Couldn't load your wish list. Please retry.",
)
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <PageBreadcrumbs
      :items="[
        { label: 'Wish list', to: '/wishlist' },
        { label: gameName, to: `/wishlist/${game}` },
        { label: heading },
      ]"
    />

    <!-- Signed out (session resolved): prompt to sign in rather than bouncing to the login
         page (matches the landing view, preserving ?redirect). While the initial session is
         still resolving, show the pending grid instead of flashing the prompt at a returning
         signed-in visitor. -->
    <CollectionSignInPrompt
      v-if="auth.sessionResolved && !auth.isAuthenticated"
      :game-name="gameName"
      list="wishlist"
    />

    <template v-else-if="auth.isAuthenticated">
      <header class="mb-4">
        <h1 class="text-3xl font-semibold tracking-tight">{{ heading }}</h1>
        <p class="text-muted-foreground mt-1 text-sm">
          <template v-if="scoped && includeRelated">
            {{ relatedCount }} related {{ setsWord }} ·
          </template>
          <template v-else-if="scoped">
            <span class="uppercase">{{ code }}</span> ·
          </template>
          <template v-if="updating">
            <UpdatingCue />
          </template>
          <template v-else>{{ countLabel }}</template>
          <!-- The scope's total wanted copies (with duplicates), shown when more copies
               than distinct cards are wanted. Hidden while searching. -->
          <template v-if="scopeCopiesLabel"> · {{ scopeCopiesLabel }}</template>
          <!-- What the scope's wishlisted cards would cost. No bulk slice (unlike the
               collection browse header) — a wish list is a shopping list, so only the
               cost matters. Hidden while searching or when nothing is priced. -->
          <template v-if="scopeTotalValue">
            ·
            <span class="text-muted-foreground text-[0.7rem] tracking-wide uppercase">Total</span>
            {{ scopeTotalValue }}
          </template>
        </p>
      </header>

      <!-- Search + sort over the (optionally set-scoped) cards. -->
      <StickySearchBar>
        <div class="flex items-center gap-2">
          <CardSearchBox v-model="searchInput" :placeholder="searchPlaceholder" class="flex-1" />
          <AdvancedSearchPanel v-model="searchInput" />
        </div>
      </StickySearchBar>
      <SearchSyntaxHint class="mt-2 mb-6" />

      <!-- Fold the set's related sub-sets (tokens, promos, decks, …) into one listing, plus
           a picker to drop into any single set — the wish-list mirror of the catalog set
           view's scope bar. Composes with show-ghosts; acting on it leaves by-drop. -->
      <SetScopeBar
        v-if="hasRelated"
        v-bind="scopeBarProps"
        @toggle="setIncludeRelated"
        @select="viewSingleSet"
      />

      <CardGridSkeleton v-if="listPending" />
      <p v-else-if="listIsError" class="text-destructive py-12">
        {{ searchError ?? errorMessage }}
      </p>

      <template v-else>
        <!-- Controls: the by-drop / all-cards toggle (drop sets) + the show-ghosts toggle,
             then the card-size + sort menus (flat views only — by-drop has a fixed order). -->
        <div
          ref="resultsTop"
          class="mb-4 flex scroll-mt-24 flex-wrap items-center justify-between gap-3"
        >
          <div class="flex flex-wrap items-center gap-2">
            <DropViewToggle v-if="scoped && hasDrops" :by-drop="byDrop" @select="setDropView" />
            <GhostToggle :show-ghosts="showGhosts" list="wishlist" @toggle="setShowGhosts" />
          </div>
          <div v-if="hasCards" class="flex gap-2">
            <CardSizeMenu />
            <CardSortMenu v-if="!byDrop" v-model="sort" :options="sortOptions" />
          </div>
        </div>

        <!-- No cards yet but a fetch is still in flight (e.g. clearing a zero-match search:
             keepPreviousData holds the empty page while the list reloads). Keep a loading
             affordance rather than flashing an "empty" state. -->
        <LoadingRow v-if="!hasCards && listIsFetching" :label="loadingLabel" />

        <!-- A search that matched nothing. -->
        <p v-else-if="!hasCards && query" class="text-muted-foreground py-12">
          No cards match “{{ query }}”.
        </p>

        <!-- Nothing to show. In show-ghosts mode that means the catalog has no cards in
             scope; otherwise it's an empty (sub-)wish-list, so offer to switch ghosts
             back on — the full card list whose add controls write to the wish list
             (unlike the catalog views, whose controls write to the collection). -->
        <div v-else-if="!hasCards" class="py-16 text-center">
          <template v-if="showGhosts">
            <p class="text-muted-foreground">
              <template v-if="scoped">No cards in {{ heading }} yet.</template>
              <template v-else>No {{ gameName }} cards found.</template>
            </p>
          </template>
          <template v-else>
            <p class="text-muted-foreground">
              <template v-if="scoped">
                You haven't wishlisted any cards from {{ heading }} yet.
              </template>
              <template v-else>Your {{ gameName }} wish list is empty.</template>
            </p>
            <button
              type="button"
              :class="buttonVariants({ variant: 'default' })"
              class="mt-4 inline-flex"
              @click="setShowGhosts(true)"
            >
              Show all cards to add some
            </button>
          </template>
        </div>

        <!-- By-drop: one section per Secret Lair drop, paginated by drop. List-only mode
             uses the collection grid (its cards are wishlisted holdings); ghost mode uses
             the catalog grid (every card in the drop) with wanted badges + dimmed
             not-wanted cards. -->
        <template v-else-if="byDrop">
          <!-- Top pager mirrors the one below (#264) so a long list can be paged from the top too. -->
          <div class="mb-6">
            <CardPagination
              v-model:page="page"
              :page-size="DROP_PAGE_SIZE"
              :total="total"
              :scroll-target="resultsTop"
            />
          </div>
          <!-- Two typed loops (not one union v-for): wishlisted drops render the
               collection grid off wish-list holdings, ghost drops the catalog grid off
               every card. -->
          <template v-if="!showGhosts">
            <DropSection
              v-for="drop in listedDropGroups"
              :key="drop.slug ?? drop.title"
              :drop="drop"
            >
              <CollectionGrid
                :game="game"
                :entries="drop.cards"
                list="wishlist"
                :owned-marks="ownedMarks"
              />
            </DropSection>
          </template>
          <template v-else>
            <DropSection
              v-for="drop in ghostDropGroups"
              :key="drop.slug ?? drop.title"
              :drop="drop"
            >
              <CardGrid
                :game="game"
                :cards="drop.cards"
                :ownership="ownership"
                :ghost-unowned="ownershipReady"
                list="wishlist"
                :owned-marks="ownedMarks"
              />
            </DropSection>
          </template>
          <div class="mt-10">
            <CardPagination
              v-model:page="page"
              :page-size="DROP_PAGE_SIZE"
              :total="total"
              :scroll-target="resultsTop"
            />
          </div>
        </template>

        <!-- Flat: one grid. List-only mode uses the collection grid (seeded quick-add
             counts); ghost mode uses the catalog grid with wanted-count badges + dimmed
             not-wanted cards. The dim waits for the counts to load (ownershipReady) so
             wishlisted cards don't flash as ghosts on first paint / a page change. -->
        <template v-else>
          <div class="mb-6">
            <CardPagination
              v-model:page="page"
              :page-size="CARD_PAGE_SIZE"
              :total="total"
              :scroll-target="resultsTop"
            />
          </div>
          <CollectionGrid
            v-if="!showGhosts"
            :game="game"
            :entries="listedEntries"
            list="wishlist"
            :owned-marks="ownedMarks"
          />
          <CardGrid
            v-else
            :game="game"
            :cards="ghostCards"
            :ownership="ownership"
            :ghost-unowned="ownershipReady"
            list="wishlist"
            :owned-marks="ownedMarks"
          />
          <div class="mt-10">
            <CardPagination
              v-model:page="page"
              :page-size="CARD_PAGE_SIZE"
              :total="total"
              :scroll-target="resultsTop"
            />
          </div>
        </template>
      </template>
    </template>

    <!-- Initial session still resolving: reserve the card grid's layout. -->
    <CardGridSkeleton v-else />
  </div>
</template>
