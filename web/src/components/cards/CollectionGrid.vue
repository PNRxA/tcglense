<script setup lang="ts">
import { computed } from 'vue'
import type { CollectionEntry } from '@/lib/api'
import CardTile from '@/components/cards/CardTile.vue'
import OwnedCountBadge from '@/components/cards/OwnedCountBadge.vue'
import { CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'

// Same density-follows-preference grid as CardGrid, but each tile carries owned-count
// badges via CardTile's #badge overlay slot (see OwnedCountBadge). Tiles still link to
// the card page, where the quantity can be edited.
defineProps<{
  game: string
  entries: CollectionEntry[]
}>()

const cardSize = useCardSizeStore()
const gridClass = computed(() => CARD_SIZE_GRID_CLASS[cardSize.size])
</script>

<template>
  <div class="grid gap-x-4 gap-y-6" :class="gridClass">
    <CardTile v-for="entry in entries" :key="entry.card.id" :game="game" :card="entry.card">
      <template #badge>
        <OwnedCountBadge :quantity="entry.quantity" :foil-quantity="entry.foil_quantity" />
      </template>
    </CardTile>
  </div>
</template>
