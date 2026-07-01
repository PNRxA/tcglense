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
import CollectionSignInPrompt from '@/components/collection/CollectionSignInPrompt.vue'
import { searchErrorMessage, useCardSearch } from '@/composables/useCardSearch'
import { useGameName } from '@/composables/useCatalog'
import { useClampPage } from '@/composables/useClampPage'
import {
  COLLECTION_PAGE_SIZE,
  useCollectionQuery,
  useCollectionSummaryQuery,
  useOwnedCounts,
} from '@/composables/useCollection'
import {
  ALL_CARDS_DEFAULT_SORT,
  ALL_CARDS_SORT_OPTIONS,
  COLLECTION_DEFAULT_SORT,
  COLLECTION_SORT_OPTIONS,
  SET_DEFAULT_SORT,
  SET_SORT_OPTIONS,
  toSortParam,
} from '@/lib/cardSort'
import { getSet, listCards, listSetCards, type Card } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'
import { cn } from '@/lib/utils'
import { useAuthStore } from '@/stores/auth'

// Owned cards for a game, either the whole collection (`/collection/:game/cards`) or
// scoped to one set (`/collection/:game/sets/:code`). The two routes share this view;
// `code` is the only difference (undefined = all cards), mirroring the catalog's
// CardsBrowseView / SetView split against one collection.
const props = defineProps<{ game: string; code?: string }>()
const game = toRef(props, 'game')
const code = toRef(props, 'code')
const setCode = computed(() => props.code || undefined)
const scoped = computed(() => !!setCode.value)

const gameName = useGameName(game)
const auth = useAuthStore()
const route = useRoute()
const router = useRouter()

// Show-ghosts mode (issue #112): when on (`?ghosts=1`), the grid also shows the cards in
// scope the user *doesn't* own — dimmed "ghosts" — so the gaps in a set (or across the
// whole game) read at a glance and can be quick-added in place. Defaults off: the
// collection normally shows only what's owned.
const showGhosts = computed(() => route.query.ghosts === '1')

function setShowGhosts(on: boolean) {
  const next = { ...route.query }
  if (on) next.ghosts = '1'
  else delete next.ghosts
  // The two modes list different cards and sort differently, so a page number and a
  // mode-specific sort don't carry across the toggle — drop both so the target mode
  // starts on page 1 at its own default order (owned = recency; ghosts = catalog order).
  delete next.page
  delete next.sort
  router.replace({ query: next })
}

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
const heading = computed(() => (scoped.value ? setName.value : 'All cards'))

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

// In show-ghosts mode the grid is really the catalog list (owned + unowned), so it offers
// the catalog's sorts — a set's collector order, or the all-cards name order — while the
// owned-only mode keeps the collection's recency-first sorts. Recency is meaningless for
// cards you don't own, so the two sort sets (and their defaults) swap with the mode; the
// getters let `useCardSearch` re-clamp the committed sort when the toggle flips.
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

// Owned-only data source (default mode). Idle while show-ghosts is on — that mode fetches
// the full catalog below instead.
const collectionQuery = useCollectionQuery(game, page, query, sort, setCode, {
  enabled: computed(() => !showGhosts.value),
})
const ownedEntries = computed(() => collectionQuery.data.value?.data ?? [])

// Show-ghosts data source: the public catalog list for this scope (owned + unowned),
// paginated + searchable + sortable exactly like the catalog browse grids. Idle unless
// show-ghosts is on.
const ghostQuery = useQuery({
  queryKey: ['collection-ghosts', game, setCode, query, sort, page],
  queryFn: () => {
    const params = {
      q: query.value || undefined,
      page: page.value,
      pageSize: COLLECTION_PAGE_SIZE,
      ...toSortParam(sort.value, defaultSort.value),
    }
    return setCode.value
      ? listSetCards(game.value, setCode.value, params)
      : listCards(game.value, params)
  },
  // Signed-in + show-ghosts only. The whole grid lives behind the signed-in template, so a
  // signed-out visitor landing on `?ghosts=1` sees the sign-in prompt — don't fetch for them.
  enabled: computed(() => auth.isAuthenticated && showGhosts.value),
  placeholderData: keepPreviousData,
})
const ghostCards = computed<Card[]>(() => ghostQuery.data.value?.data ?? [])
// The ghost list is showing a genuine, current result (not the previous page held by
// keepPreviousData) — used to gate the completion label so it isn't computed from a stale
// total while a filter/page change reloads.
const ghostSettled = computed(
  () => ghostQuery.isSuccess.value && !ghostQuery.isPlaceholderData.value,
)
// Owned counts for the visible catalog page: they drive both the owned-count badges and
// which cards render as ghosts (a card absent from the map is dimmed). `ownershipReady`
// gates the dimming so owned cards don't flash as ghosts before their counts load (an
// empty map would otherwise read as "everything unowned"). Empty/idle in the owned-only
// mode (no cards to look up).
const { ownership, ready: ownershipReady } = useOwnedCounts(game, ghostCards)

// How many distinct cards in scope the user actually owns — for the "X of Y owned"
// completion hint in show-ghosts mode. Fetched only in that mode; unfiltered by the
// search, so the hint is only shown when there's no active query.
const summaryQuery = useCollectionSummaryQuery(game, setCode, { enabled: showGhosts })
const ownedUnique = computed(() => summaryQuery.data.value?.unique_cards ?? 0)

// The active query drives the shared list state, so the template doesn't branch on mode.
const total = computed(() =>
  showGhosts.value ? (ghostQuery.data.value?.total ?? 0) : (collectionQuery.data.value?.total ?? 0),
)
const listPending = computed(() =>
  showGhosts.value ? ghostQuery.isPending.value : collectionQuery.isPending.value,
)
const listIsError = computed(() =>
  showGhosts.value ? ghostQuery.isError.value : collectionQuery.isError.value,
)
const listError = computed(() =>
  showGhosts.value ? ghostQuery.error.value : collectionQuery.error.value,
)
const listIsFetching = computed(() =>
  showGhosts.value ? ghostQuery.isFetching.value : collectionQuery.isFetching.value,
)
const listIsSuccess = computed(() =>
  showGhosts.value ? ghostQuery.isSuccess.value : collectionQuery.isSuccess.value,
)
const hasCards = computed(() =>
  showGhosts.value ? ghostCards.value.length > 0 : ownedEntries.value.length > 0,
)
// A malformed search query comes back as 422; surface its message inline.
const searchError = computed(() => searchErrorMessage(listError.value))

useClampPage(page, () => ({
  ready: listIsSuccess.value,
  total: total.value,
  pageSize: COLLECTION_PAGE_SIZE,
}))

const countLabel = computed(() => {
  const n = total.value
  const word = n === 1 ? 'card' : 'cards'
  if (query.value) return `${n.toLocaleString()} ${word} matching “${query.value}”`
  // Show-ghosts leads with completion (owned ⊆ scope). Only once both the (unfiltered)
  // summary and the ghost list have genuinely settled, and there's something in scope —
  // otherwise a mid-load `total` of 0 (or a stale filtered total) would misread as
  // "0 of 0 owned" / "N of N owned". Clamp so it can never read "N+1 of N".
  if (showGhosts.value && summaryQuery.isSuccess.value && ghostSettled.value && n > 0) {
    const owned = Math.min(ownedUnique.value, n)
    return `${owned.toLocaleString()} of ${n.toLocaleString()} owned`
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
          <template v-if="scoped">
            <span class="uppercase">{{ code }}</span> ·
          </template>
          {{ countLabel }}
        </p>
      </header>

      <!-- Search + sort over the (optionally set-scoped) cards. -->
      <div class="bg-background/85 sticky top-0 z-30 -mx-4 border-b px-4 py-3 backdrop-blur">
        <CardSearchBox v-model="searchInput" :placeholder="searchPlaceholder" />
      </div>
      <SearchSyntaxHint class="mt-2 mb-6" />

      <LoadingRow v-if="listPending" :label="loadingLabel" />
      <p v-else-if="listIsError" class="text-destructive py-12">
        {{ searchError ?? errorMessage }}
      </p>

      <template v-else>
        <!-- Controls: the show-ghosts toggle (always reachable, even with nothing owned,
             so you can reveal the whole set to fill it in), plus the card-size + sort
             menus once there are cards to arrange. -->
        <div class="mb-4 flex items-center justify-between gap-3">
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
          <div v-if="hasCards" class="flex gap-2">
            <CardSizeMenu />
            <CardSortMenu v-model="sort" :options="sortOptions" />
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

        <template v-else>
          <!-- Owned-only mode uses the collection grid (seeded quick-add counts); ghost
               mode uses the catalog grid with owned-count badges + dimmed unowned cards.
               The dim waits for ownership to load (ownershipReady) so owned cards don't
               flash as ghosts on first paint / a page change. -->
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
