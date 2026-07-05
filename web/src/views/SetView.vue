<script setup lang="ts">
import { computed, toRef } from 'vue'
import { ArrowLeft } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import AdvancedSearchPanel from '@/components/cards/AdvancedSearchPanel.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import DropSection from '@/components/cards/DropSection.vue'
import DropViewToggle from '@/components/cards/DropViewToggle.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import SearchSyntaxHint from '@/components/cards/SearchSyntaxHint.vue'
import SetScopeBar from '@/components/cards/SetScopeBar.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import { searchErrorMessage, useCardSearch } from '@/composables/useCardSearch'
import {
  CARD_PAGE_SIZE,
  DROP_PAGE_SIZE,
  useSetCardsQuery,
  useSetDropsQuery,
  useSetQuery,
} from '@/composables/useCatalog'
import { useClampPage } from '@/composables/useClampPage'
import { useOwnedCounts } from '@/composables/useCollection'
import { useSetGrouping } from '@/composables/useSetGrouping'
import { SET_DEFAULT_SORT, SET_SORT_OPTIONS } from '@/lib/cardSort'
import { type Card } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'

const props = defineProps<{ game: string; code: string }>()
const game = toRef(props, 'game')
const code = toRef(props, 'code')

// Page, search and sort live in the URL query (alongside the related/from scope), so
// they survive opening a card and pressing Back. Routing to a different set lands on
// a fresh URL, so it starts clean.
const { page, searchInput, query, sort } = useCardSearch(
  SET_DEFAULT_SORT,
  SET_SORT_OPTIONS.map((option) => option.value),
)

// Related-sub-set grouping + the "view all together" / "view just one set" scope
// nav + the by-drop view, all keyed off the (game-cached) full set list.
// `hasDrops`/`byDrop` and `setsPending` come from that same list, which the flat
// card fetch below gates on — so a drop set is known up front (no flat-grid flash,
// no throwaway fetch).
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
} = useSetGrouping(game, code)

const setQuery = useSetQuery(game, code)

const set = computed(() => setQuery.data.value)

usePageMeta({
  title: () => set.value?.name ?? code.value.toUpperCase(),
  description: () =>
    set.value
      ? `Browse cards from ${set.value.name} on TCGLense, with singles prices tracked over time.`
      : undefined,
  canonicalPath: () => `/cards/${game.value}/sets/${code.value}`,
})

const cardsQuery = useSetCardsQuery(game, code, {
  page,
  query,
  sort,
  defaultSort: SET_DEFAULT_SORT,
  includeRelated,
  // Skip the flat list while the by-drop view is active, and wait for the set
  // list to settle first — it's what tells us whether this is a drop set (and
  // resolves the related grouping), so we never fire a throwaway flat request
  // that a cold-loaded by-drop / related link would immediately discard.
  enabled: computed(() => !byDrop.value && !setsPending.value),
})

const dropsQuery = useSetDropsQuery(game, code, { page, query, enabled: byDrop })

const cards = computed(() => cardsQuery.data.value?.data ?? [])
const total = computed(() => cardsQuery.data.value?.total ?? 0)
const dropGroups = computed(() => dropsQuery.data.value?.data ?? [])
const dropTotal = computed(() => dropsQuery.data.value?.total ?? 0)

// Every card visible on the current page — the flat grid's cards, or all the drops'
// cards in the by-drop view — so a single owned-counts lookup drives the collection
// badges on whichever grid(s) render below.
const visibleCards = computed<Card[]>(() =>
  byDrop.value ? dropGroups.value.flatMap((drop) => drop.cards) : cards.value,
)
const { ownership } = useOwnedCounts(game, visibleCards)

// The list's loading / error / empty state reads from whichever query drives the
// current view. cardsQuery waits on the set list, so an as-yet-undecided drop set
// shows the active query's own pending state (no flat-grid flash), while
// keepPreviousData still carries the prior set's cards smoothly across navigation.
const listPending = computed(() =>
  byDrop.value ? dropsQuery.isPending.value : cardsQuery.isPending.value,
)
const listError = computed(() => (byDrop.value ? dropsQuery.error.value : cardsQuery.error.value))
const listIsError = computed(() =>
  byDrop.value ? dropsQuery.isError.value : cardsQuery.isError.value,
)
const isEmpty = computed(() =>
  byDrop.value ? dropGroups.value.length === 0 : cards.value.length === 0,
)

// The active view sets the pagination unit: drops (by-drop) or printings (flat).
useClampPage(page, () =>
  byDrop.value
    ? { ready: dropsQuery.isSuccess.value, total: dropTotal.value, pageSize: DROP_PAGE_SIZE }
    : { ready: cardsQuery.isSuccess.value, total: total.value, pageSize: CARD_PAGE_SIZE },
)

// When folding in related sets, the page is rooted at the group's main set.
const heading = computed(() =>
  includeRelated.value && group.value
    ? group.value.main.name
    : (set.value?.name ?? code.value.toUpperCase()),
)
const countLabel = computed(() => {
  // By-drop mode counts drops; the flat view counts card printings.
  const [n, singular] = byDrop.value ? [dropTotal.value, 'drop'] : [total.value, 'printing']
  if (!n && !query.value) return ''
  const label = `${n.toLocaleString()} ${n === 1 ? singular : `${singular}s`}`
  return query.value ? `${label} matching “${query.value}”` : label
})
// A malformed search query comes back as 422; surface its message inline.
const searchError = computed(() => searchErrorMessage(listError.value))
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <RouterLink
      :to="`/cards/${game}`"
      class="text-muted-foreground hover:text-foreground mb-4 inline-flex items-center gap-1.5 text-sm"
    >
      <ArrowLeft class="size-4" />
      All sets
    </RouterLink>

    <p v-if="setQuery.isError.value" class="text-destructive py-12">Set not found.</p>

    <template v-else>
      <header class="mb-4">
        <h1 class="text-3xl font-semibold tracking-tight">{{ heading }}</h1>
        <p class="text-muted-foreground mt-1 text-sm">
          <template v-if="includeRelated">{{ relatedCount }} related {{ setsWord }}</template>
          <template v-else>
            <span class="uppercase">{{ code }}</span>
            <template v-if="set?.set_type"> · {{ set?.set_type?.replace('_', ' ') }}</template>
          </template>
          <template v-if="countLabel"> · {{ countLabel }}</template>
        </p>
      </header>

      <!-- The search bar sticks to the top of the viewport so it stays reachable
           while scrolling a long set. -->
      <StickySearchBar>
        <div class="flex items-center gap-2">
          <CardSearchBox
            v-model="searchInput"
            :placeholder="
              includeRelated ? 'Search these sets — c:r, t:land…' : 'Search this set — c:r, t:land…'
            "
            class="flex-1"
          />
          <AdvancedSearchPanel v-model="searchInput" />
        </div>
      </StickySearchBar>
      <SearchSyntaxHint class="mt-2 mb-6" />

      <!-- Offer folding the set's related sub-sets (tokens, promos, decks, …) into
           one listing instead of visiting each individually. Shown in the by-drop
           view too (a Secret Lair-style set can still have related sub-sets), sitting
           above the by-drop/all-cards toggle — the two controls are orthogonal: this
           scopes across sub-sets, that groups the set's own cards. Folding in related
           sets is inherently a flat cross-set listing, so acting on it leaves by-drop. -->
      <SetScopeBar
        v-if="hasRelated"
        v-bind="scopeBarProps"
        @toggle="setIncludeRelated"
        @select="viewSingleSet"
      />

      <LoadingRow v-if="listPending" label="Loading cards…" />
      <p v-else-if="listIsError" class="text-destructive py-12">
        {{ searchError ?? "Couldn't load cards. Please retry." }}
      </p>
      <p v-else-if="isEmpty && query" class="text-muted-foreground py-12">
        No cards match “{{ query }}”.
      </p>
      <p v-else-if="isEmpty" class="text-muted-foreground py-12">
        No cards in {{ includeRelated ? 'these sets' : 'this set' }} yet.
      </p>

      <template v-else>
        <!-- Controls: a By-drop / All-cards toggle for drop sets, a card-size
             menu, and the sort menu (flat view only — the by-drop view has a
             fixed drop order). The size menu shows in both views since the
             by-drop sections are grids too. -->
        <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
          <DropViewToggle v-if="hasDrops" :by-drop="byDrop" @select="setDropView" />
          <span v-else />
          <div class="flex gap-2">
            <CardSizeMenu />
            <CardSortMenu v-if="!byDrop" v-model="sort" :options="SET_SORT_OPTIONS" />
          </div>
        </div>

        <!-- By-drop: one section per Secret Lair drop, paginated by drop. -->
        <template v-if="byDrop">
          <DropSection v-for="drop in dropGroups" :key="drop.slug ?? drop.title" :drop="drop">
            <CardGrid :game="game" :cards="drop.cards" :ownership="ownership" />
          </DropSection>
          <div class="mt-10">
            <CardPagination
              v-model:page="page"
              :page-size="DROP_PAGE_SIZE"
              :total="dropTotal"
              :loading="dropsQuery.isPlaceholderData.value"
            />
          </div>
        </template>

        <!-- Flat: the whole set as one collector-ordered grid. -->
        <template v-else>
          <CardGrid :game="game" :cards="cards" :ownership="ownership" />
          <div class="mt-10">
            <CardPagination
              v-model:page="page"
              :page-size="CARD_PAGE_SIZE"
              :total="total"
              :loading="cardsQuery.isPlaceholderData.value"
            />
          </div>
        </template>
      </template>
    </template>
  </div>
</template>
