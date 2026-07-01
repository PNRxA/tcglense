<script setup lang="ts">
import { computed } from 'vue'
import type { CollectionEntry } from '@/lib/api'
import CardTile from '@/components/cards/CardTile.vue'
import { CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'

// Same density-follows-preference grid as CardGrid, but each tile carries an
// owned-count badge (total copies, with the foil count called out) via CardTile's
// #badge overlay slot. Tiles still link to the card page, where the quantity can be
// edited.
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
        <span
          class="bg-primary text-primary-foreground absolute top-1.5 right-1.5 inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-xs font-semibold shadow tabular-nums"
          :title="`${entry.quantity} regular, ${entry.foil_quantity} foil`"
        >
          ×{{ entry.quantity + entry.foil_quantity }}
          <span
            v-if="entry.foil_quantity > 0"
            class="text-[0.65rem] tracking-wide uppercase opacity-80"
          >
            ✦{{ entry.foil_quantity }}
          </span>
        </span>
      </template>
    </CardTile>
  </div>
</template>
