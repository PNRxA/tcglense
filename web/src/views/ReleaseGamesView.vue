<script setup lang="ts">
import { CalendarDays, ChevronRight } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import { useGamesQuery } from '@/composables/useCatalog'
import { releasesPath } from '@/lib/releases'
import { usePageMeta } from '@/lib/seo'

// The release-calendar hub: pick a game, then see what's releasing. Mirrors CardsView /
// SealedGamesView / PreconGamesView so the section reads as part of the same catalog — same
// tile, same copy shape, same games registry — and because the nav registry expands every
// catalog item as "an all-games landing plus a row per game".
usePageMeta({
  title: 'Release calendar',
  description:
    'Upcoming and recent trading-card game releases by month — sets, Secret Lair drops, ' +
    'preconstructed decks and sealed products — with release dates and a day-before heads-up.',
  canonicalPath: '/releases',
})

const { data, isPending, isError } = useGamesQuery()
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-12">
    <header class="mb-8">
      <h1 class="text-3xl font-semibold tracking-tight">Release calendar</h1>
      <p class="text-muted-foreground mt-2">
        Pick a game to see what's coming and what just landed — sets, Secret Lair drops, decks and
        sealed products, month by month.
      </p>
    </header>

    <LoadingRow v-if="isPending" label="Loading games…" />
    <p v-else-if="isError" class="text-destructive py-12">Couldn't load games. Please retry.</p>

    <div v-else class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <RouterLink
        v-for="game in data?.data ?? []"
        :key="game.id"
        :to="releasesPath(game.id)"
        class="bg-card hover:border-ring/60 hover:bg-accent/40 group flex items-center gap-4 rounded-xl border p-5 transition-colors"
      >
        <div class="bg-muted flex size-12 shrink-0 items-center justify-center rounded-lg">
          <CalendarDays class="size-6" />
        </div>
        <div class="min-w-0 flex-1">
          <p class="font-medium">{{ game.name }}</p>
          <p class="text-muted-foreground truncate text-sm">{{ game.publisher }}</p>
          <p class="text-muted-foreground mt-1 text-xs">
            Sets, Secret Lair drops, preconstructed decks, sealed products
          </p>
        </div>
        <ChevronRight
          class="text-muted-foreground size-5 transition-transform group-hover:translate-x-0.5"
        />
      </RouterLink>
    </div>
  </div>
</template>
