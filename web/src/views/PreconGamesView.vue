<script setup lang="ts">
import { Boxes, ChevronRight } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import { useGamesQuery } from '@/composables/useCatalog'
import { preconsPath } from '@/lib/precons'
import { usePageMeta } from '@/lib/seo'

// The preconstructed-deck hub: pick a game, then browse the decklists it shipped. Mirrors
// CardsView / SealedGamesView / KeywordGamesView so the section reads as part of the same
// catalog rather than a one-off — same tile, same copy shape, same games registry.
//
// It exists because the nav registry expands every catalog item as "an all-games landing plus
// a row per game", and a precon *is* catalog data. Its per-game rows land in the deck section
// (`/decks/{game}/precons`), where the surface lives.
usePageMeta({
  title: 'Preconstructed decks',
  description:
    'Browse every preconstructed deck a trading-card game shipped — Commander decks, ' +
    'Planeswalker and Challenger decks, Jumpstart themes and Secret Lair drops — with full ' +
    'decklists, prices, and one-click copying into your own decks.',
  canonicalPath: '/precons',
})

const { data, isPending, isError } = useGamesQuery()
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-12">
    <header class="mb-8">
      <h1 class="text-3xl font-semibold tracking-tight">Preconstructed decks</h1>
      <p class="text-muted-foreground mt-2">
        Pick a game to browse the decklists it shipped — ready to read, price, or copy into your own
        decks.
      </p>
    </header>

    <LoadingRow v-if="isPending" label="Loading games…" />
    <p v-else-if="isError" class="text-destructive py-12">Couldn't load games. Please retry.</p>

    <div v-else class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <RouterLink
        v-for="game in data?.data ?? []"
        :key="game.id"
        :to="preconsPath(game.id)"
        class="bg-card hover:border-ring/60 hover:bg-accent/40 group flex items-center gap-4 rounded-xl border p-5 transition-colors"
      >
        <div class="bg-muted flex size-12 shrink-0 items-center justify-center rounded-lg">
          <Boxes class="size-6" />
        </div>
        <div class="min-w-0 flex-1">
          <p class="font-medium">{{ game.name }}</p>
          <p class="text-muted-foreground truncate text-sm">{{ game.publisher }}</p>
          <p class="text-muted-foreground mt-1 text-xs">
            Commander, Planeswalker and Challenger decks, Jumpstart, Secret Lair
          </p>
        </div>
        <ChevronRight
          class="text-muted-foreground size-5 transition-transform group-hover:translate-x-0.5"
        />
      </RouterLink>
    </div>
  </div>
</template>
