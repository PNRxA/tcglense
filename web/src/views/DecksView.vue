<script setup lang="ts">
import { RouterLink } from 'vue-router'
import { Layers } from '@lucide/vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import { useGamesQuery } from '@/composables/useCatalog'
import { usePageMeta } from '@/lib/seo'

// Per-account page — kept out of search indexes.
usePageMeta({ title: 'Your decks', canonicalPath: '/decks', noindex: true })

const { data, isPending, isError } = useGamesQuery()
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-12">
    <header class="mb-8">
      <h1 class="text-3xl font-semibold tracking-tight">Your decks</h1>
      <p class="text-muted-foreground mt-2">Pick a game to build and organise your decks.</p>
    </header>

    <LoadingRow v-if="isPending" label="Loading games…" />
    <p v-else-if="isError" class="text-destructive py-12">Couldn't load games. Please retry.</p>

    <div v-else class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <RouterLink
        v-for="game in data?.data ?? []"
        :key="game.id"
        :to="`/decks/${game.id}`"
        class="bg-card hover:border-primary/50 flex items-center gap-4 rounded-lg border p-5 transition"
      >
        <div class="bg-muted flex size-11 shrink-0 items-center justify-center rounded-lg">
          <Layers class="size-5" aria-hidden="true" />
        </div>
        <div class="min-w-0">
          <p class="truncate font-medium">{{ game.name }}</p>
          <p class="text-muted-foreground text-sm">{{ game.publisher }}</p>
        </div>
      </RouterLink>
    </div>
  </div>
</template>
