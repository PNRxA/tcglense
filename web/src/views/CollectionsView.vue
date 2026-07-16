<script setup lang="ts">
import LoadingRow from '@/components/cards/LoadingRow.vue'
import CollectionGameTile from '@/components/collection/CollectionGameTile.vue'
import { useGamesQuery } from '@/composables/useCatalog'
import { usePageMeta } from '@/lib/seo'

// Per-account page — kept out of search indexes.
usePageMeta({ title: 'Your collections', canonicalPath: '/collection', noindex: true })

const { data, isPending, isError } = useGamesQuery()
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-12">
    <header class="mb-8">
      <h1 class="text-3xl font-semibold tracking-tight">Your collections</h1>
      <p class="text-muted-foreground mt-2">
        Pick a game to see the cards and sealed products you own.
      </p>
    </header>

    <LoadingRow v-if="isPending" label="Loading games…" />
    <p v-else-if="isError" class="text-destructive py-12">Couldn't load games. Please retry.</p>

    <div v-else class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <CollectionGameTile v-for="game in data?.data ?? []" :key="game.id" :game="game" />
    </div>
  </div>
</template>
