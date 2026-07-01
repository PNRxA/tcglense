<script setup lang="ts">
import { computed } from 'vue'
import type { CollectionEntry } from '@/lib/api'
import CardTile from '@/components/cards/CardTile.vue'
import OwnedCountControl from '@/components/cards/OwnedCountControl.vue'
import { CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'

// Same density-follows-preference grid as CardGrid, but each tile carries the quick-add
// control (issue #95) seeded from the owned entry, so counts can be adjusted inline
// without opening the card page. This view only renders for signed-in users.
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
        <OwnedCountControl
          :game="game"
          :card-id="entry.card.id"
          :name="entry.card.name"
          :quantity="entry.quantity"
          :foil-quantity="entry.foil_quantity"
        />
      </template>
    </CardTile>
  </div>
</template>
