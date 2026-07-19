<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { ArrowLeft, Layers, ShoppingCart } from '@lucide/vue'
import { buttonVariants } from '@/components/ui/button'
import CardTile from '@/components/cards/CardTile.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import { useGamesQuery } from '@/composables/useCatalog'
import { useNeededCardsQuery } from '@/composables/useDecks'
import type { NeedMode } from '@/lib/api'
import { useAuthStore } from '@/stores/auth'
import { usePageMeta } from '@/lib/seo'

const props = defineProps<{ game: string }>()
const game = computed(() => props.game)
const auth = useAuthStore()

const { data: games } = useGamesQuery()
const gameName = computed(
  () => games.value?.data.find((g) => g.id === props.game)?.name ?? props.game.toUpperCase(),
)
usePageMeta({ title: computed(() => `Cards needed — ${gameName.value} decks`), noindex: true })

// The two matching modes (issue #499): by gameplay card across any printing (the default),
// or by the exact missing printing. A ref inside the query key, so switching refetches.
const mode = ref<NeedMode>('card')
const MODES: { value: NeedMode; label: string; hint: string }[] = [
  {
    value: 'card',
    label: 'By card',
    hint: 'Any printing you own covers any printing your decks want.',
  },
  {
    value: 'printing',
    label: 'Exact printing',
    hint: 'Match each deck’s exact printing against that printing in your collection.',
  },
]
const activeHint = computed(() => MODES.find((m) => m.value === mode.value)?.hint ?? '')

const neededQuery = useNeededCardsQuery(game, mode)
const needed = computed(() => neededQuery.data.value?.data ?? [])
const summaryLine = computed(() => {
  const cards = needed.value.length
  const copies = needed.value.reduce((sum, entry) => sum + entry.needed, 0)
  return `${cards} card${cards === 1 ? '' : 's'} · ${copies} cop${copies === 1 ? 'y' : 'ies'} to acquire`
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-8">
    <!-- Signed-out: prompt in place rather than bouncing to /login. -->
    <div
      v-if="auth.sessionResolved && !auth.isAuthenticated"
      class="mx-auto max-w-md py-16 text-center"
    >
      <div class="bg-muted mx-auto flex size-12 items-center justify-center rounded-lg">
        <Layers class="size-6" aria-hidden="true" />
      </div>
      <h1 class="mt-4 text-xl font-semibold">Sign in to see what your decks need</h1>
      <p class="text-muted-foreground mt-2">
        This compares every {{ gameName }} deck against your collection to show the cards you still
        need. Sign in to view it.
      </p>
      <div class="mt-6 flex justify-center gap-3">
        <RouterLink
          :class="buttonVariants()"
          :to="{ path: '/login', query: { redirect: `/decks/${game}/needed` } }"
          >Sign in</RouterLink
        >
      </div>
    </div>

    <template v-else>
      <RouterLink
        :to="`/decks/${game}`"
        class="text-muted-foreground hover:text-foreground mb-3 inline-flex items-center gap-1 text-sm"
      >
        <ArrowLeft class="size-4" /> All decks
      </RouterLink>

      <header class="mb-5 flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 class="flex items-center gap-2 text-2xl font-semibold tracking-tight">
            <ShoppingCart class="size-6" aria-hidden="true" /> Cards needed
          </h1>
          <p class="text-muted-foreground mt-1 text-sm">
            Cards your {{ gameName }} decks want beyond what your collection holds.
          </p>
        </div>

        <!-- Mode toggle: by card (any printing) vs exact printing. -->
        <div class="inline-flex rounded-lg border p-0.5" role="group" aria-label="Matching mode">
          <button
            v-for="m in MODES"
            :key="m.value"
            type="button"
            class="rounded-md px-3 py-1 text-sm transition"
            :class="
              mode === m.value
                ? 'bg-primary text-primary-foreground'
                : 'text-muted-foreground hover:text-foreground'
            "
            :aria-pressed="mode === m.value"
            @click="mode = m.value"
          >
            {{ m.label }}
          </button>
        </div>
      </header>
      <p class="text-muted-foreground -mt-3 mb-5 text-xs">{{ activeHint }}</p>

      <LoadingRow v-if="neededQuery.isPending.value" label="Checking your decks…" />
      <p v-else-if="neededQuery.isError.value" class="text-destructive py-8">
        Couldn't work out what your decks need. Please retry.
      </p>
      <p v-else-if="needed.length === 0" class="text-muted-foreground py-16 text-center">
        Your collection already covers every card across your {{ gameName }} decks. Nothing to buy!
      </p>

      <template v-else>
        <p class="text-muted-foreground mb-4 text-sm">{{ summaryLine }}</p>
        <div class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
          <div v-for="entry in needed" :key="entry.card.id">
            <CardTile :game="game" :card="entry.card">
              <template #badge>
                <span
                  class="bg-primary text-primary-foreground absolute top-1.5 right-1.5 z-20 inline-flex cursor-default items-center gap-0.5 rounded-md px-1.5 py-0.5 text-xs font-medium shadow select-none"
                  :title="`You need ${entry.needed} more (your decks want ${entry.required}, you own ${entry.owned})`"
                >
                  need {{ entry.needed }}
                </span>
              </template>
            </CardTile>
            <p class="text-muted-foreground mt-1 text-xs tabular-nums">
              want {{ entry.required }} · own {{ entry.owned }}
            </p>
            <!-- Which decks want this card (issue #499). -->
            <div class="mt-1 flex flex-wrap gap-1">
              <RouterLink
                v-for="deck in entry.decks"
                :key="deck.id"
                :to="`/decks/${game}/${deck.id}`"
                class="bg-muted text-muted-foreground hover:text-foreground max-w-full truncate rounded px-1.5 py-0.5 text-xs"
                :title="deck.name"
              >
                {{ deck.name }}
              </RouterLink>
            </div>
          </div>
        </div>
      </template>
    </template>
  </div>
</template>
