<script setup lang="ts">
import { computed, toRef } from 'vue'
import { ChevronRight, Library } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { useCollectionSummaryQuery } from '@/composables/useCollection'
import { type Game } from '@/lib/api'
import { formatUsd } from '@/lib/money'

// One game tile on the `/collection` landing. Each tile fetches that game's collection
// summary so its subtitle can show the collection's total value + its bulk (< $1/card)
// slice, instead of the static "Your <game> collection" line. The summary query is
// per-game (a composable can't be called in a v-for), so the tile owns its own fetch.
const props = defineProps<{ game: Game }>()
const gameId = toRef(() => props.game.id)

// Signed-out visitors get no summary (the authed query stays disabled), so the tile
// gracefully falls back to the static subtitle below.
const summaryQuery = useCollectionSummaryQuery(gameId)
const totalValue = computed(() => formatUsd(summaryQuery.data.value?.total_value_usd))
// Bulk is present whenever the total is (both gate on something being priced), so a
// truthy `totalValue` guarantees a `bulkValue` too.
const bulkValue = computed(() => formatUsd(summaryQuery.data.value?.bulk_value_usd))
</script>

<template>
  <RouterLink
    :to="`/collection/${game.id}`"
    class="bg-card hover:border-ring/60 hover:bg-accent/40 group flex items-center gap-4 rounded-xl border p-5 transition-colors"
  >
    <div class="bg-muted flex size-12 shrink-0 items-center justify-center rounded-lg">
      <Library class="size-6" />
    </div>
    <div class="min-w-0 flex-1">
      <p class="font-medium">{{ game.name }}</p>
      <!-- Total value + its bulk slice once the summary lands; the static line until
           then (and while signed out / nothing owned). -->
      <p v-if="totalValue" class="text-muted-foreground truncate text-sm tabular-nums">
        <span class="text-[0.7rem] tracking-wide uppercase">Total</span> {{ totalValue }} ·
        <span class="text-[0.7rem] tracking-wide uppercase">Bulk</span> {{ bulkValue }}
      </p>
      <p v-else class="text-muted-foreground truncate text-sm">Your {{ game.name }} collection</p>
    </div>
    <ChevronRight
      class="text-muted-foreground size-5 transition-transform group-hover:translate-x-0.5"
    />
  </RouterLink>
</template>
