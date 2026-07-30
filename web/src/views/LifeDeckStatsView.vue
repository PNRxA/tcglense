<script setup lang="ts">
import { computed, toRef } from 'vue'
import { Swords } from '@lucide/vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import DeckRecordTable from '@/components/life/DeckRecordTable.vue'
import LifeSignInPrompt from '@/components/life/LifeSignInPrompt.vue'
import { useGameName } from '@/composables/useCatalog'
import { useLifeDeckRecordsQuery } from '@/composables/useLifeCounter'
import { WIN_RATE_MIN_GAMES } from '@/lib/lifeLayout'
import { lifeDeckStatsPath, lifePath, toolsPath } from '@/lib/tools'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'

// How your decks are actually doing — the reason the life counter bothers with deck links.
//
// Derived entirely from finished games, so there's nothing to maintain and nothing that can drift:
// delete a game and its contribution goes with it.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')

const auth = useAuthStore()
const gameName = useGameName(game)

const { data, isPending, isError } = useLifeDeckRecordsQuery(game)
const records = computed(() => data.value?.data ?? [])

const crumbs = computed(() => [
  { label: 'Tools', to: '/tools' },
  { label: gameName.value, to: toolsPath(game.value) },
  { label: 'Life counter', to: lifePath(game.value) },
  { label: 'Deck records' },
])

usePageMeta({
  title: () => `${gameName.value} deck records`,
  canonicalPath: () => lifeDeckStatsPath(game.value),
  noindex: true,
})
</script>

<template>
  <div class="mx-auto max-w-4xl px-4 py-8">
    <PageBreadcrumbs :items="crumbs" />

    <LifeSignInPrompt v-if="auth.sessionResolved && !auth.isAuthenticated" :game-name="gameName" />

    <template v-else>
      <header class="mb-6">
        <h1 class="flex items-center gap-2 text-3xl font-semibold tracking-tight">
          <Swords class="size-7" aria-hidden="true" />
          Deck records
        </h1>
        <p class="text-muted-foreground mt-2">
          Every {{ gameName }} deck you've linked to a tracked game, and how it has done. A win rate
          is only quoted once a deck has {{ WIN_RATE_MIN_GAMES }} games behind it.
        </p>
      </header>

      <LoadingRow v-if="isPending" label="Loading records…" />
      <p v-else-if="isError" class="text-destructive py-12">Couldn't load records. Please retry.</p>

      <div v-else-if="records.length" class="bg-card rounded-xl border p-4">
        <DeckRecordTable :records="records" :game="game" />
      </div>

      <div v-else class="bg-card rounded-xl border p-8 text-center">
        <p class="font-medium">No records yet</p>
        <p class="text-muted-foreground mx-auto mt-2 max-w-sm text-sm">
          Link a player to one of your decks when you start a game, then record who won. Finished
          games are what these records are built from.
        </p>
      </div>
    </template>
  </div>
</template>
