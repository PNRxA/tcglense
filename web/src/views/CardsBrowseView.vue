<script setup lang="ts">
import { computed, onUnmounted, ref, toRef, watch } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { Loader2, Search } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { Input } from '@/components/ui/input'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import { listCards, listGames } from '@/lib/api'

const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')

const PAGE_SIZE = 60
const page = ref(1)
const searchInput = ref('')
const query = ref('')

// Debounce typing into the search box, and snap back to page 1 on a new query.
let timer: ReturnType<typeof setTimeout> | undefined
watch(searchInput, (value) => {
  clearTimeout(timer)
  timer = setTimeout(() => {
    query.value = value.trim()
    page.value = 1
  }, 300)
})
onUnmounted(() => clearTimeout(timer))

// Switching games (same route component, different :game) starts fresh.
watch(game, () => {
  page.value = 1
  searchInput.value = ''
  query.value = ''
})

const gamesQuery = useQuery({
  queryKey: ['games'],
  queryFn: () => listGames(),
  staleTime: Infinity,
})
const gameName = computed(
  () =>
    gamesQuery.data.value?.data.find((g) => g.id === game.value)?.name ?? game.value.toUpperCase(),
)

const cardsQuery = useQuery({
  queryKey: ['cards', game, query, page],
  queryFn: () =>
    listCards(game.value, {
      q: query.value || undefined,
      page: page.value,
      pageSize: PAGE_SIZE,
    }),
  placeholderData: keepPreviousData,
  staleTime: 5 * 60 * 1000,
})

const cards = computed(() => cardsQuery.data.value?.data ?? [])
const total = computed(() => cardsQuery.data.value?.total ?? 0)
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

    <header class="mb-6 flex flex-wrap items-center justify-between gap-4">
      <h1 class="text-3xl font-semibold tracking-tight">All cards</h1>
      <div class="relative w-full sm:w-72">
        <Search
          class="text-muted-foreground pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2"
        />
        <Input v-model="searchInput" placeholder="Search cards…" class="pl-9" />
      </div>
    </header>

    <p class="text-muted-foreground mb-6 text-sm">
      <template v-if="cardsQuery.isFetching.value && !cards.length">Searching…</template>
      <template v-else>{{ total.toLocaleString() }} {{ total === 1 ? 'card' : 'cards' }}</template>
      <template v-if="query"> matching “{{ query }}”</template>
    </p>

    <div
      v-if="cardsQuery.isPending.value"
      class="text-muted-foreground flex items-center gap-2 py-12"
    >
      <Loader2 class="size-4 animate-spin" />
      Loading cards…
    </div>
    <p v-else-if="cardsQuery.isError.value" class="text-destructive py-12">
      Couldn't load cards. Please retry.
    </p>
    <p v-else-if="!cards.length" class="text-muted-foreground py-12">No cards found.</p>

    <template v-else>
      <CardGrid :game="game" :cards="cards" />
      <div class="mt-10">
        <CardPagination v-model:page="page" :page-size="PAGE_SIZE" :total="total" />
      </div>
    </template>
  </div>
</template>
