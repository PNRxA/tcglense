<script setup lang="ts">
import { computed, toRef } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { Ghost } from '@lucide/vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import CollectionGrid from '@/components/cards/CollectionGrid.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import SearchSyntaxHint from '@/components/cards/SearchSyntaxHint.vue'
import SetScopeBar from '@/components/cards/SetScopeBar.vue'
import CollectionSignInPrompt from '@/components/collection/CollectionSignInPrompt.vue'
import { searchErrorMessage, useCardSearch } from '@/composables/useCardSearch'
import { useGameName } from '@/composables/useCatalog'
import { useClampPage } from '@/composables/useClampPage'
import {
  COLLECTION_DROP_PAGE_SIZE,
  COLLECTION_PAGE_SIZE,
  useCollectionDropsQuery,
  useCollectionQuery,
  useCollectionSummaryQuery,
  useOwnedCounts,
} from '@/composables/useCollection'
import { useSetGrouping } from '@/composables/useSetGrouping'
import {
  ALL_CARDS_DEFAULT_SORT,
  ALL_CARDS_SORT_OPTIONS,
  COLLECTION_DEFAULT_SORT,
  COLLECTION_SORT_OPTIONS,
  SET_DEFAULT_SORT,
  SET_SORT_OPTIONS,
  toSortParam,
} from '@/lib/cardSort'
import { getSet, listCards, listSetCards, listSetDrops, type Card } from '@/lib/api'
import { formatUsd } from '@/lib/money'
import { usePageMeta } from '@/lib/seo'
import { cn } from '@/lib/utils'
import { useAuthStore } from '@/stores/auth'

// Owned cards for a game, either the whole collection (`/collection/:game/cards`) or
// scoped to one set (`/collection/:game/sets/:code`). The two routes share this view;
// `code` is the only difference (undefined = all cards), mirroring the catalog's
// CardsBrowseView / SetView split against one collection.
//
// Three composable view controls layer on top, mirroring the catalog set view but scoped
// to what you own: **show-ghosts** (#112 — also reveal unowned cards, dimmed), **by-drop**
// (#113 — group a Secret Lair-style set into Scryfall's drops) and **include-related**
// (#113 — fold a set's related sub-sets into one listing). They compose into a
// {owned, ghost} × {flat, by-drop} matrix, with include-related a flat scope expansion.
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

// Show-ghosts mode (issue #112): when on (`?ghosts=1`), the grid also shows the cards in
// scope the user *doesn't* own — dimmed "ghosts" — so the gaps in a set (or across the
// whole game) read at a glance and can be quick-added in place. Defaults off: the
// collection normally shows only what's owned. It composes with by-drop and include-related.
const showGhosts = computed(() => route.query.ghosts === '1')

function setShowGhosts(on: boolean) {
  const next = { ...route.query }
  if (on) next.ghosts = '1'
  else delete next.ghosts
  // The two modes list different cards and sort differently, so a page number and a
  // mode-specific sort don't carry across the toggle — drop both so the target mode
  // starts on page 1 at its own default order (owned = recency; ghosts = catalog order).
  // The by-drop / include-related scope (view / related / from) is preserved.
  delete next.page
  delete next.sort
  router.replace({ query: next })
}

// Related-sub-set grouping + the "view all together" scope nav + `hasDrops`, all keyed
// off the (game-cached) public set list — reused from the catalog set view, but pointed
// at the collection's own routes.
const {
  group,
  isMainSet,
  relatedCount,
  hasRelated,
  includeRelated,
  memberOptions,
  activeSetCode,
  originName,
  hasDrops,
  setsPending,
  listState,
  setIncludeRelated,
  viewSingleSet,
} = useSetGrouping(game, groupCode, { basePath: '/collection', preserveQuery: ['ghosts'] })

// A set's display name for the header/breadcrumb (public, cached). Only fetched for the
// set-scoped view; falls back to the upper-cased code until it loads or if it's unknown.
const setQuery = useQuery({
  queryKey: ['set', game, code],
  queryFn: () => getSet(game.value, code.value as string),
  enabled: scoped,
})
const setName = computed(() =>
  scoped.value ? (setQuery.data.value?.name ?? code.value?.toUpperCase() ?? '') : '',
)
// When folding in related sets, the heading is rooted at the group's main set.
const heading = computed(() => {
  if (!scoped.value) return 'All cards'
  return includeRelated.value && group.value ? group.value.main.name : setName.value
})
const setsWord = computed(() => (relatedCount.value === 1 ? 'set' : 'sets'))

// Per-account page — kept out of search indexes.
usePageMeta({
  title: () =>
    scoped.value
      ? `${setName.value} — your ${gameName.value} collection`
      : `All your ${gameName.value} cards`,
  canonicalPath: () =>
    scoped.value
      ? `/collection/${game.value}/sets/${code.value}`
      : `/collection/${game.value}/cards`,
  noindex: true,
})

// By-drop is the default for a drop-grouped set; ?view=all opts back into the flat grid,
// and the include-related view (?related=1) is itself a flat cross-set listing, so it
// suppresses by-drop. Only ever active in the set-scoped view; composes with show-ghosts
// (owned drops vs. the catalog's every-card drops). `hasDrops` comes from the game-cached
// set list, so it's known up front — no flat-grid flash.
const byDrop = computed(
  () => scoped.value && hasDrops.value && route.query.view !== 'all' && !includeRelated.value,
)

// In show-ghosts mode the flat grid is really the catalog list (owned + unowned), so it
// offers the catalog's sorts — a set's collector order, or the all-cards name order — while
// the owned-only mode keeps the collection's recency-first sorts. Recency is meaningless for
// cards you don't own, so the two sort sets (and their defaults) swap with the mode; the
// getters let `useCardSearch` re-clamp the committed sort when the toggle flips. (By-drop
// hides the sort menu — a fixed drop order — so its sort set is moot there.)
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

// ---- Four data sources: {owned, ghost} × {flat, by-drop}. Exactly one is enabled. ----

// Owned + flat (the default). Idle when ghosts or by-drop is active.
const collectionQuery = useCollectionQuery(game, page, query, sort, setCode, {
  includeRelated,
  enabled: computed(() => !showGhosts.value && !byDrop.value && flatReady.value),
})
const ownedEntries = computed(() => collectionQuery.data.value?.data ?? [])

// Owned + by drop: the user's owned cards grouped into Secret Lair drops.
const ownedDropsQuery = useCollectionDropsQuery(game, groupCode, page, query, {
  enabled: computed(() => !showGhosts.value && byDrop.value),
})
const ownedDropGroups = computed(() => ownedDropsQuery.data.value?.data ?? [])

// Ghost + flat: the public catalog list for this scope (owned + unowned), paginated +
// searchable + sortable exactly like the catalog browse grids, spanning the set's group
// when include-related is on.
const ghostQuery = useQuery({
  queryKey: ['collection-ghosts', game, setCode, includeRelated, query, sort, page],
  queryFn: () => {
    const params = {
      q: query.value || undefined,
      page: page.value,
      pageSize: COLLECTION_PAGE_SIZE,
      ...toSortParam(sort.value, defaultSort.value),
    }
    return setCode.value
      ? listSetCards(game.value, setCode.value, {
          ...params,
          includeRelated: includeRelated.value || undefined,
        })
      : listCards(game.value, params)
  },
  // Signed-in + show-ghosts + flat only (and, when scoped, once the set list has settled).
  // The whole grid lives behind the signed-in template, so a signed-out visitor landing on
  // `?ghosts=1` sees the sign-in prompt — don't fetch for them.
  enabled: computed(
    () => auth.isAuthenticated && showGhosts.value && !byDrop.value && flatReady.value,
  ),
  placeholderData: keepPreviousData,
})
const ghostCards = computed<Card[]>(() => ghostQuery.data.value?.data ?? [])
// The ghost list is showing a genuine, current result (not the previous page held by
// keepPreviousData) — used to gate the completion label so it isn't computed from a stale
// total while a filter/page change reloads.
const ghostSettled = computed(
  () => ghostQuery.isSuccess.value && !ghostQuery.isPlaceholderData.value,
)

// Ghost + by drop: the catalog's by-drop endpoint (every card in each drop), so the drops
// show what you're missing (dimmed) alongside what you own.
const ghostDropsQuery = useQuery({
  queryKey: ['collection-ghost-drops', game, setCode, query, page],
  queryFn: () =>
    listSetDrops(game.value, setCode.value as string, {
      q: query.value || undefined,
      page: page.value,
      pageSize: COLLECTION_DROP_PAGE_SIZE,
    }),
  enabled: computed(() => auth.isAuthenticated && showGhosts.value && byDrop.value),
  placeholderData: keepPreviousData,
})
const ghostDropGroups = computed(() => ghostDropsQuery.data.value?.data ?? [])

// Owned counts for the visible ghost cards (the flat page, or every drop's cards): they
// drive both the owned-count badges and which cards render as ghosts (a card absent from
// the map is dimmed). `ownershipReady` gates the dimming so owned cards don't flash as
// ghosts before their counts load. Empty/idle in the owned-only modes.
const ghostVisibleCards = computed<Card[]>(() =>
  byDrop.value ? ghostDropGroups.value.flatMap((drop) => drop.cards) : ghostCards.value,
)
const { ownership, ready: ownershipReady } = useOwnedCounts(game, ghostVisibleCards)

// The owned stats for the current scope (all cards / a set / a set + its related group,
// tracking `includeRelated`), unfiltered by the search box. Fetched in every mode: it
// drives the scoped collection **value** shown next to the count (issue #119) and the
// scope's "X/Y owned" completion count (issue #125 — the show-ghosts view reads it as
// owned-of-catalog; the owned-only view reads owned-of-`scopeTotal`). Because it now spans
// the group under include-related, both read correctly there too. (Reuses the landing's
// cache key, so arriving from `/collection/:game` is a cache hit for the all-cards scope.)
const summaryQuery = useCollectionSummaryQuery(game, setCode, { includeRelated })
const ownedUnique = computed(() => summaryQuery.data.value?.unique_cards ?? 0)

// The catalog total of cards in the current scope — the denominator for the set-scoped
// owned-only "X/Y owned" completion count (issue #125). A single set uses its own
// card_count; include-related sums the whole group (root + related sub-sets), mirroring what
// the ghost list counts. Null off a set scope (the whole-game "all cards" view has no
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
// The scope's owned value, formatted (null while loading or when nothing in scope is
// priced). Shown only when there's no active search — the value is the whole scope's,
// so pairing it with a search-filtered count would misread.
const scopeValueLabel = computed(() =>
  query.value ? null : formatUsd(summaryQuery.data.value?.total_value_usd),
)

// ---- Active-mode selectors, so the template doesn't branch on mode for state. ----
const total = computed(() => {
  if (showGhosts.value) {
    return byDrop.value
      ? (ghostDropsQuery.data.value?.total ?? 0)
      : (ghostQuery.data.value?.total ?? 0)
  }
  return byDrop.value
    ? (ownedDropsQuery.data.value?.total ?? 0)
    : (collectionQuery.data.value?.total ?? 0)
})
const listPending = computed(() => {
  if (showGhosts.value) {
    return byDrop.value ? ghostDropsQuery.isPending.value : ghostQuery.isPending.value
  }
  return byDrop.value ? ownedDropsQuery.isPending.value : collectionQuery.isPending.value
})
const listIsError = computed(() => {
  if (showGhosts.value) {
    return byDrop.value ? ghostDropsQuery.isError.value : ghostQuery.isError.value
  }
  return byDrop.value ? ownedDropsQuery.isError.value : collectionQuery.isError.value
})
const listError = computed(() => {
  if (showGhosts.value) {
    return byDrop.value ? ghostDropsQuery.error.value : ghostQuery.error.value
  }
  return byDrop.value ? ownedDropsQuery.error.value : collectionQuery.error.value
})
const listIsFetching = computed(() => {
  if (showGhosts.value) {
    return byDrop.value ? ghostDropsQuery.isFetching.value : ghostQuery.isFetching.value
  }
  return byDrop.value ? ownedDropsQuery.isFetching.value : collectionQuery.isFetching.value
})
const listIsSuccess = computed(() => {
  if (showGhosts.value) {
    return byDrop.value ? ghostDropsQuery.isSuccess.value : ghostQuery.isSuccess.value
  }
  return byDrop.value ? ownedDropsQuery.isSuccess.value : collectionQuery.isSuccess.value
})
const hasCards = computed(() => {
  if (showGhosts.value) {
    return byDrop.value ? ghostDropGroups.value.length > 0 : ghostCards.value.length > 0
  }
  return byDrop.value ? ownedDropGroups.value.length > 0 : ownedEntries.value.length > 0
})
// A malformed search query comes back as 422; surface its message inline.
const searchError = computed(() => searchErrorMessage(listError.value))

// The active view sets the pagination unit: drops (by-drop) or printings (flat).
const pageSize = computed(() => (byDrop.value ? COLLECTION_DROP_PAGE_SIZE : COLLECTION_PAGE_SIZE))
useClampPage(page, () => ({
  ready: listIsSuccess.value,
  total: total.value,
  pageSize: pageSize.value,
}))

// Toggle the by-drop vs flat view of this set. Preserves the search + sort and the
// show-ghosts mode, but sheds the related/from scope and restarts paging — the two views
// paginate over different units. ?view=all marks the flat mode; by-drop is the default.
function setView(mode: 'drops' | 'all') {
  const next = listState()
  if (showGhosts.value) next.ghosts = '1'
  if (mode === 'all') next.view = 'all'
  router.replace({ query: next })
}

// A slash-form "X/Y owned" set-completion count (issue #125), shared by both flat modes:
// owned-only reads what you have of the set's total, show-ghosts what you have of the catalog
// scope. Clamped so a paper-only vs. Scryfall card-count skew can never read "N+1 of N".
function completionLabel(owned: number, scopeSize: number) {
  return `${Math.min(owned, scopeSize).toLocaleString()}/${scopeSize.toLocaleString()} owned`
}

const countLabel = computed(() => {
  const n = total.value
  // By-drop counts drops.
  if (byDrop.value) {
    const label = `${n.toLocaleString()} ${n === 1 ? 'drop' : 'drops'}`
    return query.value ? `${label} matching “${query.value}”` : label
  }
  const word = n === 1 ? 'card' : 'cards'
  if (query.value) return `${n.toLocaleString()} ${word} matching “${query.value}”`
  // Show-ghosts (flat) leads with completion (owned ⊆ scope): `n` is the catalog total in
  // scope, `ownedUnique` how many you own. Only once both the (unfiltered) summary and the
  // ghost list have genuinely settled, and there's something in scope — otherwise a mid-load
  // `total` of 0 (or a stale filtered total) would misread. The summary spans the same
  // set/group the ghost list does (it tracks include-related), so this reads right there too.
  if (showGhosts.value) {
    if (summaryQuery.isSuccess.value && ghostSettled.value && n > 0) {
      return completionLabel(ownedUnique.value, n)
    }
    return `${n.toLocaleString()} ${word}`
  }
  // Owned-only, set-scoped: the same "X/Y owned" completion, but `n` is now the owned count
  // and `scopeTotal` the set/group's catalog total — so the browse header matches the landing
  // tiles. Gated on the owned list settling so it doesn't flash "0/Y" while loading; the
  // whole-game "all cards" view has no scope total, so it keeps the plain "N cards".
  if (scoped.value && listIsSuccess.value && scopeTotal.value != null && scopeTotal.value > 0) {
    return completionLabel(n, scopeTotal.value)
  }
  return `${n.toLocaleString()} ${word}`
})

const searchPlaceholder = computed(() => {
  if (scoped.value) return 'Search this set — name, c:r, t:goblin…'
  return showGhosts.value
    ? 'Search all cards — name, c:r, t:goblin…'
    : 'Search your collection — name, c:r, t:goblin…'
})

// The ghost data source is the public catalog, so its loading/error copy is neutral
// ("cards"); the owned-only mode keeps the collection-worded copy.
const loadingLabel = computed(() => (showGhosts.value ? 'Loading cards…' : 'Loading your cards…'))
const errorMessage = computed(() =>
  showGhosts.value
    ? "Couldn't load cards. Please retry."
    : "Couldn't load your collection. Please retry.",
)
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <nav class="text-muted-foreground mb-4 text-sm">
      <RouterLink to="/collection" class="hover:underline">Collection</RouterLink>
      <span class="mx-1.5">/</span>
      <RouterLink :to="`/collection/${game}`" class="hover:underline">{{ gameName }}</RouterLink>
      <span class="mx-1.5">/</span>
      <span class="text-foreground">{{ heading }}</span>
    </nav>

    <!-- Signed out: the collection routes are public, so prompt to sign in rather than
         bouncing to the login page (matches the landing view, preserving ?redirect). -->
    <CollectionSignInPrompt v-if="!auth.isAuthenticated" :game-name="gameName" />

    <template v-else>
      <header class="mb-4">
        <h1 class="text-3xl font-semibold tracking-tight">{{ heading }}</h1>
        <p class="text-muted-foreground mt-1 text-sm">
          <template v-if="scoped && includeRelated">
            {{ relatedCount }} related {{ setsWord }} ·
          </template>
          <template v-else-if="scoped">
            <span class="uppercase">{{ code }}</span> ·
          </template>
          {{ countLabel }}
          <!-- The scope's owned value (issue #119): what your cards in this set / group /
               whole collection are worth. Hidden while searching or when nothing is priced. -->
          <template v-if="scopeValueLabel"> · {{ scopeValueLabel }}</template>
        </p>
      </header>

      <!-- Search + sort over the (optionally set-scoped) cards. -->
      <div class="bg-background/85 sticky top-0 z-30 -mx-4 border-b px-4 py-3 backdrop-blur">
        <CardSearchBox v-model="searchInput" :placeholder="searchPlaceholder" />
      </div>
      <SearchSyntaxHint class="mt-2 mb-6" />

      <!-- Fold the set's related sub-sets (tokens, promos, decks, …) into one listing, plus
           a picker to drop into any single set — the collection mirror of the catalog set
           view's scope bar. Composes with show-ghosts; acting on it leaves by-drop. -->
      <SetScopeBar
        v-if="scoped && hasRelated"
        :include-related="includeRelated"
        :is-main-set="isMainSet"
        :main-name="group?.main.name ?? ''"
        :related-count="relatedCount"
        :sets-word="setsWord"
        :member-options="memberOptions"
        :active-set-code="activeSetCode"
        :origin-name="originName"
        @toggle="setIncludeRelated"
        @select="viewSingleSet"
      />

      <LoadingRow v-if="listPending" :label="loadingLabel" />
      <p v-else-if="listIsError" class="text-destructive py-12">
        {{ searchError ?? errorMessage }}
      </p>

      <template v-else>
        <!-- Controls: the by-drop / all-cards toggle (drop sets) + the show-ghosts toggle,
             then the card-size + sort menus (flat views only — by-drop has a fixed order). -->
        <div class="mb-4 flex items-center justify-between gap-3">
          <div class="flex flex-wrap items-center gap-2">
            <div
              v-if="scoped && hasDrops"
              class="bg-muted text-muted-foreground inline-flex rounded-md p-0.5 text-sm"
            >
              <button
                type="button"
                :class="
                  cn(
                    'rounded px-3 py-1 font-medium transition-colors',
                    byDrop ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
                  )
                "
                @click="setView('drops')"
              >
                By drop
              </button>
              <button
                type="button"
                :class="
                  cn(
                    'rounded px-3 py-1 font-medium transition-colors',
                    !byDrop ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
                  )
                "
                @click="setView('all')"
              >
                All cards
              </button>
            </div>
            <button
              type="button"
              :class="
                cn(
                  'inline-flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-sm font-medium transition-colors',
                  showGhosts
                    ? 'border-primary bg-primary/10 text-foreground'
                    : 'text-muted-foreground hover:text-foreground',
                )
              "
              :aria-pressed="showGhosts"
              title="Also show cards you don't own, dimmed, to see the gaps"
              @click="setShowGhosts(!showGhosts)"
            >
              <Ghost class="size-4" aria-hidden="true" />
              Show ghosts
            </button>
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
             scope; otherwise it's an empty (sub-)collection, so point at the catalog. -->
        <div v-else-if="!hasCards" class="py-16 text-center">
          <template v-if="showGhosts">
            <p class="text-muted-foreground">
              <template v-if="scoped">No cards in {{ heading }} yet.</template>
              <template v-else>No {{ gameName }} cards found.</template>
            </p>
          </template>
          <template v-else>
            <p class="text-muted-foreground">
              <template v-if="scoped">You don't own any cards from {{ heading }} yet.</template>
              <template v-else>Your {{ gameName }} collection is empty.</template>
            </p>
            <RouterLink
              :to="scoped ? `/cards/${game}/sets/${code}` : `/cards/${game}/cards`"
              :class="buttonVariants({ variant: 'default' })"
              class="mt-4 inline-flex"
            >
              Browse cards to add some
            </RouterLink>
          </template>
        </div>

        <!-- By-drop: one section per Secret Lair drop, paginated by drop. Owned mode uses
             the collection grid (its cards are owned holdings); ghost mode uses the catalog
             grid (every card in the drop) with owned badges + dimmed unowned cards. -->
        <template v-else-if="byDrop">
          <template v-if="!showGhosts">
            <section
              v-for="drop in ownedDropGroups"
              :id="drop.slug ?? undefined"
              :key="drop.slug ?? drop.title"
              class="mb-10 scroll-mt-20"
            >
              <div class="mb-4 flex items-baseline gap-2 border-b pb-2">
                <h2 class="text-lg font-semibold tracking-tight">{{ drop.title }}</h2>
                <span class="text-muted-foreground text-sm tabular-nums">
                  {{ drop.card_count }} {{ drop.card_count === 1 ? 'card' : 'cards' }}
                </span>
              </div>
              <CollectionGrid :game="game" :entries="drop.cards" />
            </section>
          </template>
          <template v-else>
            <section
              v-for="drop in ghostDropGroups"
              :id="drop.slug ?? undefined"
              :key="drop.slug ?? drop.title"
              class="mb-10 scroll-mt-20"
            >
              <div class="mb-4 flex items-baseline gap-2 border-b pb-2">
                <h2 class="text-lg font-semibold tracking-tight">{{ drop.title }}</h2>
                <span class="text-muted-foreground text-sm tabular-nums">
                  {{ drop.card_count }} {{ drop.card_count === 1 ? 'card' : 'cards' }}
                </span>
              </div>
              <CardGrid
                :game="game"
                :cards="drop.cards"
                :ownership="ownership"
                :ghost-unowned="ownershipReady"
              />
            </section>
          </template>
          <div class="mt-10">
            <CardPagination
              v-model:page="page"
              :page-size="COLLECTION_DROP_PAGE_SIZE"
              :total="total"
            />
          </div>
        </template>

        <!-- Flat: one grid. Owned-only mode uses the collection grid (seeded quick-add
             counts); ghost mode uses the catalog grid with owned-count badges + dimmed
             unowned cards. The dim waits for ownership to load (ownershipReady) so owned
             cards don't flash as ghosts on first paint / a page change. -->
        <template v-else>
          <CollectionGrid v-if="!showGhosts" :game="game" :entries="ownedEntries" />
          <CardGrid
            v-else
            :game="game"
            :cards="ghostCards"
            :ownership="ownership"
            :ghost-unowned="ownershipReady"
          />
          <div class="mt-10">
            <CardPagination v-model:page="page" :page-size="COLLECTION_PAGE_SIZE" :total="total" />
          </div>
        </template>
      </template>
    </template>
  </div>
</template>
