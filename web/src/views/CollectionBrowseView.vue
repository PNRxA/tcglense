<script setup lang="ts">
import { computed, toRef } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
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
} from '@/composables/useCollection'
import { useSetGrouping } from '@/composables/useSetGrouping'
import { COLLECTION_DEFAULT_SORT, COLLECTION_SORT_OPTIONS } from '@/lib/cardSort'
import { getSet } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'
import { cn } from '@/lib/utils'
import { useAuthStore } from '@/stores/auth'

// Owned cards for a game, either the whole collection (`/collection/:game/cards`) or
// scoped to one set (`/collection/:game/sets/:code`). The two routes share this view;
// `code` is the only difference (undefined = all cards), mirroring the catalog's
// CardsBrowseView / SetView split against one collection. The set-scoped view carries the
// same two toggles the catalog set view has — "by drop" for a Secret Lair-style set and
// "include related sets" — scoped to what the user owns.
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
} = useSetGrouping(game, groupCode, { basePath: '/collection' })

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

// Page, search and sort live in the URL query (like the catalog browse views), so they
// survive opening a card and pressing Back and are shareable/reload-safe.
const { page, searchInput, query, sort } = useCardSearch(
  COLLECTION_DEFAULT_SORT,
  COLLECTION_SORT_OPTIONS.map((option) => option.value),
)

// By-drop is the default for a drop-grouped owned set; ?view=all opts back into the flat
// grid, and the related-sets view (?related=1) is itself a flat listing, so it suppresses
// by-drop too. Only ever active in the set-scoped view. (`hasDrops` comes from the
// game-cached set list, so it's known up front — no flat-grid flash.)
const byDrop = computed(
  () => scoped.value && hasDrops.value && route.query.view !== 'all' && !includeRelated.value,
)

// The flat owned-card list. Gated off while by-drop is active, and (in the scoped view)
// until the set list settles — it's what tells us whether this is a drop set, so we don't
// fire a throwaway flat request a cold-loaded by-drop link would immediately discard. The
// unscoped view has no drops/related, so it never waits on the set list.
const flatEnabled = computed(() => !byDrop.value && (!scoped.value || !setsPending.value))
const collectionQuery = useCollectionQuery(game, page, query, sort, setCode, {
  includeRelated,
  enabled: flatEnabled,
})
const dropsQuery = useCollectionDropsQuery(game, groupCode, page, query, { enabled: byDrop })

const entries = computed(() => collectionQuery.data.value?.data ?? [])
const total = computed(() => collectionQuery.data.value?.total ?? 0)
const dropGroups = computed(() => dropsQuery.data.value?.data ?? [])
const dropTotal = computed(() => dropsQuery.data.value?.total ?? 0)

// The list's loading / error / empty state reads from whichever query drives the current
// view (mirrors the catalog set view).
const listPending = computed(() =>
  byDrop.value ? dropsQuery.isPending.value : collectionQuery.isPending.value,
)
const listFetching = computed(() =>
  byDrop.value ? dropsQuery.isFetching.value : collectionQuery.isFetching.value,
)
const listError = computed(() =>
  byDrop.value ? dropsQuery.error.value : collectionQuery.error.value,
)
const listIsError = computed(() =>
  byDrop.value ? dropsQuery.isError.value : collectionQuery.isError.value,
)
const isEmpty = computed(() =>
  byDrop.value ? dropGroups.value.length === 0 : entries.value.length === 0,
)
// A malformed search query comes back as 422; surface its message inline.
const searchError = computed(() => searchErrorMessage(listError.value))

// The active view sets the pagination unit: drops (by-drop) or printings (flat).
useClampPage(page, () =>
  byDrop.value
    ? {
        ready: dropsQuery.isSuccess.value,
        total: dropTotal.value,
        pageSize: COLLECTION_DROP_PAGE_SIZE,
      }
    : {
        ready: collectionQuery.isSuccess.value,
        total: total.value,
        pageSize: COLLECTION_PAGE_SIZE,
      },
)

// Toggle the by-drop vs flat view of this set. Preserves the search + sort (via
// listState) but sheds the related/from scope and restarts paging — the two views
// paginate over different units. ?view=all marks the flat mode; by-drop is the default.
function setView(mode: 'drops' | 'all') {
  const next = listState()
  if (mode === 'all') next.view = 'all'
  router.replace({ query: next })
}

const countLabel = computed(() => {
  // By-drop mode counts drops; every other view counts owned cards.
  const [n, singular] = byDrop.value ? [dropTotal.value, 'drop'] : [total.value, 'card']
  const label = `${n.toLocaleString()} ${n === 1 ? singular : `${singular}s`}`
  return query.value ? `${label} matching “${query.value}”` : label
})
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
        </p>
      </header>

      <!-- Search + sort over the (optionally set-scoped) owned cards. -->
      <div class="bg-background/85 sticky top-0 z-30 -mx-4 border-b px-4 py-3 backdrop-blur">
        <CardSearchBox
          v-model="searchInput"
          :placeholder="
            scoped
              ? 'Search this set — name, c:r, t:goblin…'
              : 'Search your collection — name, c:r, t:goblin…'
          "
        />
      </div>
      <SearchSyntaxHint class="mt-2 mb-6" />

      <!-- Offer folding the set's related sub-sets (tokens, promos, decks, …) into one
           owned listing, plus a picker to drop into any single set in the group — the
           collection mirror of the catalog set view's scope bar. Shown in the by-drop
           view too; acting on it leaves by-drop (a group listing is inherently flat). -->
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

      <LoadingRow v-if="listPending" label="Loading your cards…" />
      <p v-else-if="listIsError" class="text-destructive py-12">
        {{ searchError ?? "Couldn't load your collection. Please retry." }}
      </p>

      <!-- A search that matched nothing. -->
      <p v-else-if="isEmpty && query" class="text-muted-foreground py-12">
        No cards match “{{ query }}”.
      </p>

      <!-- No results but a fetch is still in flight (e.g. clearing a zero-match search:
           keepPreviousData holds the empty page while the unscoped list reloads). Keep a
           loading affordance rather than flashing the "empty" state below. -->
      <LoadingRow v-else-if="isEmpty && listFetching" label="Loading your cards…" />

      <!-- Nothing owned in this scope (e.g. a direct link to a set you own nothing in). -->
      <div v-else-if="isEmpty" class="py-16 text-center">
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
      </div>

      <template v-else>
        <!-- Controls: a By-drop / All-cards toggle for drop sets, a card-size menu, and
             the sort menu (flat view only — the by-drop view has a fixed drop order). -->
        <div class="mb-4 flex items-center justify-between gap-3">
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
          <span v-else />
          <div class="flex gap-2">
            <CardSizeMenu />
            <CardSortMenu v-if="!byDrop" v-model="sort" :options="COLLECTION_SORT_OPTIONS" />
          </div>
        </div>

        <!-- By-drop: one section per Secret Lair drop, paginated by drop. -->
        <template v-if="byDrop">
          <section
            v-for="drop in dropGroups"
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
          <div class="mt-10">
            <CardPagination
              v-model:page="page"
              :page-size="COLLECTION_DROP_PAGE_SIZE"
              :total="dropTotal"
            />
          </div>
        </template>

        <!-- Flat: the whole (optionally group-spanning) owned scope as one grid. -->
        <template v-else>
          <CollectionGrid :game="game" :entries="entries" />
          <div class="mt-10">
            <CardPagination v-model:page="page" :page-size="COLLECTION_PAGE_SIZE" :total="total" />
          </div>
        </template>
      </template>
    </template>
  </div>
</template>
