<script setup lang="ts">
import { computed } from 'vue'
import type { Card } from '@/lib/api'
import CardTile from '@/components/cards/CardTile.vue'
import { CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'

defineProps<{
  game: string
  cards: Card[]
}>()

// The grid's column count follows the user's persisted size preference (set via
// CardSizeMenu); fewer columns render larger cards. The store reads localStorage
// synchronously, so the right density is applied on the first paint.
const cardSize = useCardSizeStore()
const gridClass = computed(() => CARD_SIZE_GRID_CLASS[cardSize.size])
</script>

<template>
  <div class="grid gap-x-4 gap-y-6" :class="gridClass">
    <CardTile v-for="card in cards" :key="card.id" :game="game" :card="card" />
  </div>
</template>
