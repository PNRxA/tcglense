<script setup lang="ts">
import { computed } from 'vue'
import { Layers, Sparkles } from '@lucide/vue'

// The owned-count overlay shown on a card image: a total-copies chip (stacked-cards
// icon, regular + foil) and, when any are foil, a separate foil chip (sparkles).
// Shared by the collection grid and the public browse grids (issue #85). The parent
// is expected to be `relative` (CardTile's badge slot is), since this positions
// itself absolutely in the corner.
const props = defineProps<{
  quantity: number
  foilQuantity: number
}>()

const total = computed(() => props.quantity + props.foilQuantity)
</script>

<template>
  <!-- `z-20` keeps the chips above the card, which lifts to `z-10` on hover (see
    CardTile); without it the enlarged card paints over the badges and they vanish
    while hovered. -->
  <div class="absolute top-1.5 right-1.5 z-20 flex items-center gap-1">
    <span
      v-if="total > 0"
      class="bg-primary text-primary-foreground inline-flex items-center gap-0.5 rounded-md px-1.5 py-0.5 text-xs font-semibold shadow tabular-nums"
      :title="`${total} total`"
    >
      <Layers class="size-3" aria-hidden="true" />
      {{ total }}
    </span>
    <span
      v-if="foilQuantity > 0"
      class="bg-primary text-primary-foreground inline-flex items-center gap-0.5 rounded-md px-1.5 py-0.5 text-xs font-semibold shadow tabular-nums"
      :title="`${foilQuantity} foil`"
    >
      <Sparkles class="size-3" aria-hidden="true" />
      {{ foilQuantity }}
    </span>
  </div>
</template>
