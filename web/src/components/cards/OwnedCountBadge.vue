<script setup lang="ts">
import { computed } from 'vue'
import { Layers, Sparkles } from '@lucide/vue'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

// The owned-count overlay shown on a card image: a total-copies chip (stacked-cards
// icon, regular + foil) and, when any are foil, a separate foil chip (sparkles).
// Shared by the collection grid and the public browse grids (issue #85). The parent
// is expected to be `relative` (CardTile's badge slot is), since this positions
// itself absolutely in the corner. Each chip carries a shadcn tooltip spelling out
// what its icon means ("N total" / "N foil"), with a matching `aria-label` so the
// count is announced to screen readers even though the chip isn't focusable (issue #94).
const props = defineProps<{
  quantity: number
  foilQuantity: number
}>()

const total = computed(() => props.quantity + props.foilQuantity)
</script>

<template>
  <!-- `z-20` keeps the chips above the card, which lifts to `z-10` on hover (see
    CardTile); without it the enlarged card paints over the badges and they vanish
    while hovered. The tooltip content is portalled to <body>, so it's unaffected. -->
  <div class="absolute top-1.5 right-1.5 z-20 flex items-center gap-1">
    <Tooltip v-if="total > 0">
      <TooltipTrigger as-child>
        <span
          class="bg-primary text-primary-foreground inline-flex items-center gap-0.5 rounded-md px-1.5 py-0.5 text-xs font-semibold shadow tabular-nums"
          :aria-label="`${total} total`"
        >
          <Layers class="size-3" aria-hidden="true" />
          {{ total }}
        </span>
      </TooltipTrigger>
      <TooltipContent>{{ total }} total</TooltipContent>
    </Tooltip>
    <Tooltip v-if="foilQuantity > 0">
      <TooltipTrigger as-child>
        <span
          class="bg-primary text-primary-foreground inline-flex items-center gap-0.5 rounded-md px-1.5 py-0.5 text-xs font-semibold shadow tabular-nums"
          :aria-label="`${foilQuantity} foil`"
        >
          <Sparkles class="size-3" aria-hidden="true" />
          {{ foilQuantity }}
        </span>
      </TooltipTrigger>
      <TooltipContent>{{ foilQuantity }} foil</TooltipContent>
    </Tooltip>
  </div>
</template>
