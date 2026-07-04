<script setup lang="ts">
import { ChevronRight, Package } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import { useGamesQuery } from '@/composables/useCatalog'
import { usePageMeta } from '@/lib/seo'

// The sealed-products landing (/sealed): pick a game to browse its sealed products.
// Mirrors CardsView's game-picker grid, one tier up from SealedBrowseView.
usePageMeta({
  title: 'Sealed products',
  description:
    'Browse sealed trading-card products — booster boxes, bundles and decks — with ' +
    'current prices and price history on TCGLense.',
  canonicalPath: '/sealed',
})

const { data, isPending, isError } = useGamesQuery()
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-12">
    <header class="mb-8">
      <h1 class="text-3xl font-semibold tracking-tight">Sealed products</h1>
      <p class="text-muted-foreground mt-2">Pick a game to browse its sealed products.</p>
    </header>

    <LoadingRow v-if="isPending" label="Loading games…" />
    <p v-else-if="isError" class="text-destructive py-12">Couldn't load games. Please retry.</p>

    <div v-else class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <RouterLink
        v-for="game in data?.data ?? []"
        :key="game.id"
        :to="`/sealed/${game.id}`"
        class="bg-card hover:border-ring/60 hover:bg-accent/40 group flex items-center gap-4 rounded-xl border p-5 transition-colors"
      >
        <div class="bg-muted flex size-12 shrink-0 items-center justify-center rounded-lg">
          <Package class="size-6" />
        </div>
        <div class="min-w-0 flex-1">
          <p class="font-medium">{{ game.name }}</p>
          <p class="text-muted-foreground truncate text-sm">{{ game.publisher }}</p>
          <p class="text-muted-foreground mt-1 text-xs">Data by {{ game.data_source }}</p>
        </div>
        <ChevronRight
          class="text-muted-foreground size-5 transition-transform group-hover:translate-x-0.5"
        />
      </RouterLink>
    </div>
  </div>
</template>
