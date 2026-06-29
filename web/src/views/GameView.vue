<script setup lang="ts">
import { computed, toRef, watch } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { LayoutGrid, Loader2 } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import SetTile from '@/components/cards/SetTile.vue'
import { gameStatus, listGames, listSets } from '@/lib/api'

const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')

const gamesQuery = useQuery({
  queryKey: ['games'],
  queryFn: () => listGames(),
  staleTime: Infinity,
})
const gameName = computed(
  () =>
    gamesQuery.data.value?.data.find((g) => g.id === game.value)?.name ?? game.value.toUpperCase(),
)

const statusQuery = useQuery({
  queryKey: ['status', game],
  queryFn: () => gameStatus(game.value),
  // Poll while an import is in progress; stop once it finishes or fails.
  refetchInterval: (query) => {
    const status = query.state.data?.status
    return status === 'complete' || status === 'error' ? false : 4000
  },
})

const setsQuery = useQuery({
  queryKey: ['sets', game],
  queryFn: () => listSets(game.value),
  staleTime: 5 * 60 * 1000,
})

// When the import finishes, pull the freshly-populated sets.
watch(
  () => statusQuery.data.value?.status,
  (status, previous) => {
    if (status === 'complete' && previous && previous !== 'complete') {
      setsQuery.refetch()
    }
  },
)

const importing = computed(() => {
  const status = statusQuery.data.value?.status
  return status !== undefined && status !== 'complete' && status !== 'error'
})
const sets = computed(() => setsQuery.data.value?.data ?? [])
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <nav class="text-muted-foreground mb-4 text-sm">
      <RouterLink to="/cards" class="hover:underline">Cards</RouterLink>
      <span class="mx-1.5">/</span>
      <span class="text-foreground">{{ gameName }}</span>
    </nav>

    <header class="mb-6 flex flex-wrap items-end justify-between gap-4">
      <div>
        <h1 class="text-3xl font-semibold tracking-tight">{{ gameName }}</h1>
        <p class="text-muted-foreground mt-1">{{ sets.length }} sets</p>
      </div>
      <RouterLink :to="`/cards/${game}/cards`" :class="buttonVariants({ variant: 'default' })">
        <LayoutGrid />
        View all cards
      </RouterLink>
    </header>

    <!-- First-boot import progress. -->
    <div
      v-if="importing"
      class="bg-muted/50 text-muted-foreground mb-6 flex items-center gap-3 rounded-lg border p-4 text-sm"
    >
      <Loader2 class="size-4 shrink-0 animate-spin" />
      <span>
        Importing card data…
        <template v-if="statusQuery.data.value?.cards_imported">
          {{ statusQuery.data.value.cards_imported.toLocaleString() }} cards so far.
        </template>
        This page will update automatically.
      </span>
    </div>

    <div
      v-if="setsQuery.isPending.value"
      class="text-muted-foreground flex items-center gap-2 py-12"
    >
      <Loader2 class="size-4 animate-spin" />
      Loading sets…
    </div>
    <p v-else-if="setsQuery.isError.value" class="text-destructive py-12">
      Couldn't load sets. Please retry.
    </p>
    <p v-else-if="!sets.length && !importing" class="text-muted-foreground py-12">
      No sets available yet.
    </p>

    <div v-else class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
      <SetTile v-for="set in sets" :key="set.code" :game="game" :set="set" />
    </div>
  </div>
</template>
