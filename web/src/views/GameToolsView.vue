<script setup lang="ts">
import { computed, toRef } from 'vue'
import { ChevronRight } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import { useGameName } from '@/composables/useCatalog'
import { toolPath, toolsFor, toolsPath } from '@/lib/tools'
import { usePageMeta } from '@/lib/seo'

// One game's tools. Driven by the `lib/tools.ts` registry, so a second tool is a data entry
// rather than a page edit — and a one-tool index still reads native, since the tile grid is the
// same one the catalog landings use.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')

const gameName = useGameName(game)
const tools = computed(() => toolsFor(game.value))

const crumbs = computed(() => [
  { label: 'Home', to: '/' },
  { label: 'Tools', to: '/tools' },
  { label: gameName.value },
])

usePageMeta({
  title: () => `${gameName.value} tools`,
  description: () =>
    `Play aids for ${gameName.value} — count life at the table, keep each game's ` +
    'history, and see how your decks are doing.',
  canonicalPath: () => toolsPath(game.value),
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-12">
    <PageBreadcrumbs :items="crumbs" />
    <header class="mb-8">
      <h1 class="text-3xl font-semibold tracking-tight">{{ gameName }} tools</h1>
      <p class="text-muted-foreground mt-2">Play aids for use at the table.</p>
    </header>

    <div v-if="tools.length" class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <RouterLink
        v-for="tool in tools"
        :key="tool.slug"
        :to="toolPath(game, tool.slug)"
        class="bg-card hover:border-ring/60 hover:bg-accent/40 group flex items-center gap-4 rounded-xl border p-5 transition-colors"
      >
        <div class="bg-muted flex size-12 shrink-0 items-center justify-center rounded-lg">
          <component :is="tool.icon" class="size-6" />
        </div>
        <div class="min-w-0 flex-1">
          <p class="font-medium">{{ tool.name }}</p>
          <p class="text-muted-foreground mt-1 text-sm">{{ tool.blurb }}</p>
        </div>
        <ChevronRight
          class="text-muted-foreground size-5 shrink-0 transition-transform group-hover:translate-x-0.5"
        />
      </RouterLink>
    </div>
    <p v-else class="text-muted-foreground py-12">No tools for {{ gameName }} yet.</p>

    <p v-if="tools.length" class="text-muted-foreground mt-6 text-sm">More tools are on the way.</p>
  </div>
</template>
