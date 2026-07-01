<script setup lang="ts">
import { computed } from 'vue'
import { Layers, Sparkles } from '@lucide/vue'
import type { CollectionEntry } from '@/lib/api'
import CardTile from '@/components/cards/CardTile.vue'
import { CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'

// Same density-follows-preference grid as CardGrid, but each tile carries owned-count
// badges via CardTile's #badge overlay slot: the total count (stacked-cards icon,
// regular + foil) and, when any are foil, the foil count (sparkles icon). Tiles still
// link to the card page, where the quantity can be edited.
defineProps<{
  game: string
  entries: CollectionEntry[]
}>()

const cardSize = useCardSizeStore()
const gridClass = computed(() => CARD_SIZE_GRID_CLASS[cardSize.size])

// The stacked-cards badge shows every owned copy, foils included.
const totalOwned = (entry: CollectionEntry) => entry.quantity + entry.foil_quantity
</script>

<template>
  <div class="grid gap-x-4 gap-y-6" :class="gridClass">
    <CardTile v-for="entry in entries" :key="entry.card.id" :game="game" :card="entry.card">
      <template #badge>
        <!-- `z-20` keeps the owned-count chips above the card, which lifts to
          `z-10` on hover (see CardTile); without it the enlarged card paints over
          the badges and they vanish while hovered. -->
        <div class="absolute top-1.5 right-1.5 z-20 flex items-center gap-1">
          <span
            v-if="totalOwned(entry) > 0"
            class="bg-primary text-primary-foreground inline-flex items-center gap-0.5 rounded-md px-1.5 py-0.5 text-xs font-semibold shadow tabular-nums"
            :title="`${totalOwned(entry)} total`"
          >
            <Layers class="size-3" aria-hidden="true" />
            {{ totalOwned(entry) }}
          </span>
          <span
            v-if="entry.foil_quantity > 0"
            class="bg-primary text-primary-foreground inline-flex items-center gap-0.5 rounded-md px-1.5 py-0.5 text-xs font-semibold shadow tabular-nums"
            :title="`${entry.foil_quantity} foil`"
          >
            <Sparkles class="size-3" aria-hidden="true" />
            {{ entry.foil_quantity }}
          </span>
        </div>
      </template>
    </CardTile>
  </div>
</template>
