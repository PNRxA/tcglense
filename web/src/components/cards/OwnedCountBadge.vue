<script setup lang="ts">
import { computed } from 'vue'
import { Layers, Sparkles } from '@lucide/vue'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

// The owned-count chips shown on a card image: a total-copies chip (stacked-cards icon,
// regular + foil) and, when any are foil, a separate foil chip (sparkles). Shared by the
// collection grid and the public browse grids (issue #85), now rendered inside
// OwnedCountControl's trigger (which owns the corner positioning — bottom-left, per issue
// #100). Each chip carries a matching `aria-label` so the count is announced to screen
// readers (issue #94), and — when `tooltip` is on — a shadcn tooltip spelling out what its
// icon means. `tooltip` is turned off when the badge is itself a popover trigger, so a
// hover tooltip doesn't fight the click-to-open panel (and TooltipTrigger doesn't nest
// inside PopoverTrigger).
const props = withDefaults(
  defineProps<{
    quantity: number
    foilQuantity: number
    tooltip?: boolean
  }>(),
  { tooltip: true },
)

const total = computed(() => props.quantity + props.foilQuantity)

// One shared chip style; `tabular-nums` keeps counts from jittering as they change.
const chipClass =
  'bg-primary text-primary-foreground inline-flex items-center gap-0.5 rounded-md px-1.5 py-0.5 text-xs font-semibold shadow tabular-nums'
</script>

<template>
  <div class="inline-flex items-center gap-1">
    <template v-if="total > 0">
      <Tooltip v-if="tooltip">
        <TooltipTrigger as-child>
          <span :class="chipClass" :aria-label="`${total} total`">
            <Layers class="size-3" aria-hidden="true" />
            {{ total }}
          </span>
        </TooltipTrigger>
        <TooltipContent>{{ total }} total</TooltipContent>
      </Tooltip>
      <span v-else :class="chipClass" :aria-label="`${total} total`">
        <Layers class="size-3" aria-hidden="true" />
        {{ total }}
      </span>
    </template>

    <template v-if="foilQuantity > 0">
      <Tooltip v-if="tooltip">
        <TooltipTrigger as-child>
          <span :class="chipClass" :aria-label="`${foilQuantity} foil`">
            <Sparkles class="size-3" aria-hidden="true" />
            {{ foilQuantity }}
          </span>
        </TooltipTrigger>
        <TooltipContent>{{ foilQuantity }} foil</TooltipContent>
      </Tooltip>
      <span v-else :class="chipClass" :aria-label="`${foilQuantity} foil`">
        <Sparkles class="size-3" aria-hidden="true" />
        {{ foilQuantity }}
      </span>
    </template>
  </div>
</template>
