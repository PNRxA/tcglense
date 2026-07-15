<script setup lang="ts">
import { computed, ref, watchEffect } from 'vue'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import DeckStatBars from '@/components/decks/DeckStatBars.vue'
import type { DeckCardEntry } from '@/lib/api'
import { calculateDeckStats, drawProbability } from '@/lib/deckStats'

const props = defineProps<{ cards: DeckCardEntry[] }>()
const stats = computed(() => calculateDeckStats(props.cards))
const selectedCard = ref('')
const cardsSeen = ref(7)
const maxCardsSeen = computed(() => Math.max(1, Math.min(30, stats.value.totalCopies)))

watchEffect(() => {
  const options = stats.value.cardOdds
  if (!options.some((item) => item.name === selectedCard.value)) {
    selectedCard.value = options[0]?.name ?? ''
  }
  cardsSeen.value = Math.min(Math.max(1, cardsSeen.value), maxCardsSeen.value)
})

const selectedCopies = computed(
  () => stats.value.cardOdds.find((item) => item.name === selectedCard.value)?.copies ?? 0,
)
const selectedProbability = computed(() =>
  drawProbability(stats.value.totalCopies, selectedCopies.value, cardsSeen.value),
)
const probabilityLabel = computed(
  () => `${(selectedProbability.value * 100).toFixed(1).replace('.0', '')}%`,
)
</script>

<template>
  <Card v-if="stats.totalCopies > 0" class="mb-6">
    <CardHeader>
      <CardTitle class="text-base">Deck analytics</CardTitle>
    </CardHeader>
    <CardContent class="space-y-6">
      <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <div class="bg-muted/50 rounded-md p-3">
          <p class="text-muted-foreground text-xs">Copies</p>
          <p class="mt-1 text-xl font-semibold tabular-nums">{{ stats.totalCopies }}</p>
        </div>
        <div class="bg-muted/50 rounded-md p-3">
          <p class="text-muted-foreground text-xs">Unique printings</p>
          <p class="mt-1 text-xl font-semibold tabular-nums">{{ stats.uniqueCards }}</p>
        </div>
        <div class="bg-muted/50 rounded-md p-3">
          <p class="text-muted-foreground text-xs">Average mana value</p>
          <p class="mt-1 text-xl font-semibold tabular-nums">
            {{ stats.averageManaValue?.toFixed(2) ?? '—' }}
          </p>
        </div>
        <div class="bg-muted/50 rounded-md p-3">
          <p class="text-muted-foreground text-xs">Lands</p>
          <p class="mt-1 text-xl font-semibold tabular-nums">{{ stats.landCopies }}</p>
        </div>
      </div>

      <div class="grid gap-6 md:grid-cols-3">
        <DeckStatBars title="Mana curve (nonlands)" :items="stats.manaCurve" />
        <DeckStatBars title="Colour identity" :items="stats.colors" />
        <DeckStatBars title="Card types" :items="stats.cardTypes" />
      </div>

      <section class="border-t pt-5">
        <h3 class="text-sm font-semibold">Draw probability</h3>
        <p class="text-muted-foreground mt-1 text-xs">
          Chance of seeing at least one copy without replacement.
        </p>
        <div
          class="mt-3 grid gap-4 sm:grid-cols-[minmax(0,1fr)_minmax(12rem,1fr)_auto] sm:items-end"
        >
          <label class="space-y-1.5 text-sm">
            <span class="block text-xs font-medium">Card</span>
            <Select v-model="selectedCard">
              <SelectTrigger class="w-full" aria-label="Card for draw probability">
                <SelectValue placeholder="Choose a card" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem v-for="item in stats.cardOdds" :key="item.name" :value="item.name">
                  {{ item.name }} ({{ item.copies }})
                </SelectItem>
              </SelectContent>
            </Select>
          </label>
          <label class="space-y-1.5 text-sm">
            <span class="flex justify-between gap-2 text-xs font-medium">
              Cards seen <span class="tabular-nums">{{ cardsSeen }}</span>
            </span>
            <input
              v-model.number="cardsSeen"
              type="range"
              min="1"
              :max="maxCardsSeen"
              class="accent-primary h-9 w-full"
            />
          </label>
          <div class="bg-primary/10 min-w-24 rounded-md px-4 py-2 text-center">
            <p class="text-primary text-2xl font-semibold tabular-nums">{{ probabilityLabel }}</p>
            <p class="text-muted-foreground text-xs">at least one</p>
          </div>
        </div>
      </section>
    </CardContent>
  </Card>
</template>
