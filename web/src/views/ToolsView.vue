<script setup lang="ts">
import { computed } from 'vue'
import { ChevronRight, Wrench } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import { useGamesQuery } from '@/composables/useCatalog'
import { toolsFor, toolsPath } from '@/lib/tools'
import { usePageMeta } from '@/lib/seo'

// The tools hub: pick a game, then pick a play aid.
//
// Mirrors KeywordGamesView (which mirrors CardsView) so the section reads as part of the same
// site rather than a bolt-on — same wrapper, same tile, same copy shape. Only games that
// actually have a tool are listed: a tile leading to an empty index would be a dead end.
usePageMeta({
  title: 'Trading card game tools',
  description:
    'Play aids for your games — a life counter that tracks life totals, keeps the ' +
    'gain/loss history, and builds a win record for your decks.',
  canonicalPath: '/tools',
})

const { data, isPending, isError } = useGamesQuery()
const games = computed(() =>
  (data.value?.data ?? [])
    .map((game) => ({ ...game, tools: toolsFor(game.id) }))
    .filter((game) => game.tools.length > 0),
)
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-12">
    <header class="mb-8">
      <h1 class="text-3xl font-semibold tracking-tight">Tools</h1>
      <p class="text-muted-foreground mt-2">
        Play aids that live alongside the catalog. Pick a game to see what's available.
      </p>
    </header>

    <LoadingRow v-if="isPending" label="Loading games…" />
    <p v-else-if="isError" class="text-destructive py-12">Couldn't load games. Please retry.</p>

    <div v-else-if="games.length" class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <RouterLink
        v-for="game in games"
        :key="game.id"
        :to="toolsPath(game.id)"
        class="bg-card hover:border-ring/60 hover:bg-accent/40 group flex items-center gap-4 rounded-xl border p-5 transition-colors"
      >
        <div class="bg-muted flex size-12 shrink-0 items-center justify-center rounded-lg">
          <Wrench class="size-6" />
        </div>
        <div class="min-w-0 flex-1">
          <p class="font-medium">{{ game.name }}</p>
          <p class="text-muted-foreground truncate text-sm">{{ game.publisher }}</p>
          <p class="text-muted-foreground mt-1 text-xs">
            {{ game.tools.map((tool) => tool.name).join(', ') }}
          </p>
        </div>
        <ChevronRight
          class="text-muted-foreground size-5 transition-transform group-hover:translate-x-0.5"
        />
      </RouterLink>
    </div>

    <p v-else class="text-muted-foreground py-12">No tools available yet.</p>
  </div>
</template>
