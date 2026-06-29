<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { ArrowLeft, Loader2 } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import { getSet, listSetCards } from '@/lib/api'

const props = defineProps<{ game: string; code: string }>()
const game = toRef(props, 'game')
const code = toRef(props, 'code')

const PAGE_SIZE = 60
const page = ref(1)
// Reset to the first page when navigating between sets.
watch(code, () => {
  page.value = 1
})

const setQuery = useQuery({
  queryKey: ['set', game, code],
  queryFn: () => getSet(game.value, code.value),
  staleTime: 5 * 60 * 1000,
})

const cardsQuery = useQuery({
  queryKey: ['set-cards', game, code, page],
  queryFn: () => listSetCards(game.value, code.value, { page: page.value, pageSize: PAGE_SIZE }),
  placeholderData: keepPreviousData,
  staleTime: 5 * 60 * 1000,
})

const set = computed(() => setQuery.data.value)
const cards = computed(() => cardsQuery.data.value?.data ?? [])
const total = computed(() => cardsQuery.data.value?.total ?? 0)
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
      <header class="mb-6">
        <h1 class="text-3xl font-semibold tracking-tight">
          {{ set?.name ?? code.toUpperCase() }}
        </h1>
        <p class="text-muted-foreground mt-1 text-sm">
          <span class="uppercase">{{ code }}</span>
          <template v-if="set?.set_type"> · {{ set?.set_type?.replace('_', ' ') }}</template>
          <template v-if="total"> · {{ total.toLocaleString() }} printings</template>
        </p>
      </header>

      <div
        v-if="cardsQuery.isPending.value"
        class="text-muted-foreground flex items-center gap-2 py-12"
      >
        <Loader2 class="size-4 animate-spin" />
        Loading cards…
      </div>
      <p v-else-if="!cards.length" class="text-muted-foreground py-12">No cards in this set yet.</p>

      <template v-else>
        <CardGrid :game="game" :cards="cards" />
        <div class="mt-10">
          <CardPagination v-model:page="page" :page-size="PAGE_SIZE" :total="total" />
        </div>
      </template>
    </template>
  </div>
</template>
