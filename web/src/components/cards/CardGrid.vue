<script setup lang="ts">
import { computed } from 'vue'
import type { Card, OwnedCountsMap } from '@/lib/api'
import CardTile from '@/components/cards/CardTile.vue'
import OwnedCountBadge from '@/components/cards/OwnedCountBadge.vue'
import { CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'

defineProps<{
  game: string
  cards: Card[]
  // Owned counts keyed by card id (from `useOwnedCounts`): when a card appears here an
  // owned-count badge is overlaid on its image. Omitted for signed-out visitors and
  // non-collection grids, which then render no badges.
  ownership?: OwnedCountsMap
}>()

// The grid's column count follows the user's persisted size preference (set via
// CardSizeMenu); fewer columns render larger cards. The store reads localStorage
// synchronously, so the right density is applied on the first paint.
const cardSize = useCardSizeStore()
const gridClass = computed(() => CARD_SIZE_GRID_CLASS[cardSize.size])
</script>

<template>
  <div class="grid gap-x-4 gap-y-6" :class="gridClass">
    <CardTile v-for="card in cards" :key="card.id" :game="game" :card="card">
      <template #badge>
        <OwnedCountBadge
          v-if="ownership?.[card.id]"
          :quantity="ownership?.[card.id]?.quantity ?? 0"
          :foil-quantity="ownership?.[card.id]?.foil_quantity ?? 0"
        />
      </template>
    </CardTile>
  </div>
</template>
