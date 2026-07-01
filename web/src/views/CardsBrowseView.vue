<script setup lang="ts">
import { computed, toRef } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { RouterLink } from 'vue-router'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import AdvancedSearchPanel from '@/components/cards/AdvancedSearchPanel.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import SearchSyntaxHint from '@/components/cards/SearchSyntaxHint.vue'
import { searchErrorMessage, useCardSearch } from '@/composables/useCardSearch'
import { useGameName } from '@/composables/useCatalog'
import { useClampPage } from '@/composables/useClampPage'
import { useOwnedCounts } from '@/composables/useCollection'
import { ALL_CARDS_DEFAULT_SORT, ALL_CARDS_SORT_OPTIONS, toSortParam } from '@/lib/cardSort'
import { listCards } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'

const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')

const PAGE_SIZE = 60
// Page, search and sort live in the URL query, so they survive opening a card and
// pressing Back. Switching games routes to a fresh URL, so it starts clean.
const { page, searchInput, query, sort } = useCardSearch(
  ALL_CARDS_DEFAULT_SORT,
  ALL_CARDS_SORT_OPTIONS.map((option) => option.value),
)

const gameName = useGameName(game)

usePageMeta({
  title: () => `All ${gameName.value} cards`,
  description: () =>
    `Search and browse every ${gameName.value} card tracked on TCGLense, with current prices.`,
  canonicalPath: () => `/cards/${game.value}/cards`,
})

const cardsQuery = useQuery({
  queryKey: ['cards', game, query, sort, page],
  queryFn: () =>
    listCards(game.value, {
      q: query.value || undefined,
      page: page.value,
      pageSize: PAGE_SIZE,
      ...toSortParam(sort.value, ALL_CARDS_DEFAULT_SORT),
    }),
  placeholderData: keepPreviousData,
})

const cards = computed(() => cardsQuery.data.value?.data ?? [])
const total = computed(() => cardsQuery.data.value?.total ?? 0)
// Owned-count badges for signed-in users, overlaid on the grid below.
const { ownership } = useOwnedCounts(game, cards)
// A malformed search query comes back as 422; surface its message inline.
const searchError = computed(() => searchErrorMessage(cardsQuery.error.value))

useClampPage(page, () => ({
  ready: cardsQuery.isSuccess.value,
  total: total.value,
  pageSize: PAGE_SIZE,
}))
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <nav class="text-muted-foreground mb-4 text-sm">
      <RouterLink to="/cards" class="hover:underline">Cards</RouterLink>
      <span class="mx-1.5">/</span>
      <RouterLink :to="`/cards/${game}`" class="hover:underline">{{ gameName }}</RouterLink>
      <span class="mx-1.5">/</span>
      <span class="text-foreground">All cards</span>
    </nav>

    <h1 class="mb-4 text-3xl font-semibold tracking-tight">All cards</h1>

    <!-- The search bar sticks to the top of the viewport so it stays reachable
         while scrolling a long results grid. -->
    <StickySearchBar>
      <div class="flex items-center gap-2">
        <CardSearchBox
          v-model="searchInput"
          placeholder="Search — name, c:r, t:goblin…"
          class="flex-1"
        />
        <AdvancedSearchPanel v-model="searchInput" />
      </div>
    </StickySearchBar>
    <SearchSyntaxHint class="mt-2" />

    <p class="text-muted-foreground mt-4 mb-6 text-sm">
      <template v-if="cardsQuery.isFetching.value && !cards.length">Searching…</template>
      <template v-else>{{ total.toLocaleString() }} {{ total === 1 ? 'card' : 'cards' }}</template>
      <template v-if="query"> matching “{{ query }}”</template>
    </p>

    <LoadingRow v-if="cardsQuery.isPending.value" label="Loading cards…" />
    <p v-else-if="cardsQuery.isError.value" class="text-destructive py-12">
      {{ searchError ?? "Couldn't load cards. Please retry." }}
    </p>
    <p v-else-if="!cards.length" class="text-muted-foreground py-12">No cards found.</p>

    <template v-else>
      <div class="mb-4 flex flex-wrap justify-end gap-2">
        <CardSizeMenu />
        <CardSortMenu v-model="sort" :options="ALL_CARDS_SORT_OPTIONS" />
      </div>
      <CardGrid :game="game" :cards="cards" :ownership="ownership" />
      <div class="mt-10">
        <CardPagination v-model:page="page" :page-size="PAGE_SIZE" :total="total" />
      </div>
    </template>
  </div>
</template>
