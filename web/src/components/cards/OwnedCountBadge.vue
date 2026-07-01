<script setup lang="ts">
import { computed, type Component } from 'vue'
import { Layers, Plus, Sparkles } from '@lucide/vue'
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
//
// `hoverAsAdd` (issue #136): when the badge sits inside a quick-add trigger, hovering or
// keyboard-focusing that trigger swaps each chip's leading icon for a "+" to signal that
// clicking adds more copies — the count stays put. The trigger tags itself `group/add`; the
// swap is pure CSS off `group-hover/add` / `group-focus-within/add`, so it costs no
// reactivity and follows focus for keyboard users. Off by default (a plain, static chip).
const props = withDefaults(
  defineProps<{
    quantity: number
    foilQuantity: number
    tooltip?: boolean
    hoverAsAdd?: boolean
  }>(),
  { tooltip: true, hoverAsAdd: false },
)

const total = computed(() => props.quantity + props.foilQuantity)

// The chips to render: always the total (when any are owned), plus a foil chip when some
// copies are foil. Each names the icon that leads it and the `aria-label` that voices it.
const chips = computed(() => {
  const list: { key: string; icon: Component; count: number; label: string }[] = []
  if (total.value > 0) {
    list.push({ key: 'total', icon: Layers, count: total.value, label: `${total.value} total` })
  }
  if (props.foilQuantity > 0) {
    list.push({
      key: 'foil',
      icon: Sparkles,
      count: props.foilQuantity,
      label: `${props.foilQuantity} foil`,
    })
  }
  return list
})

// One shared chip style; `tabular-nums` keeps counts from jittering as they change.
const chipClass =
  'bg-primary text-primary-foreground inline-flex items-center gap-0.5 rounded-md px-1.5 py-0.5 text-xs font-semibold shadow tabular-nums'
</script>

<template>
  <div class="inline-flex items-center gap-1">
    <template v-for="chip in chips" :key="chip.key">
      <Tooltip v-if="tooltip">
        <TooltipTrigger as-child>
          <span :class="chipClass" :aria-label="chip.label">
            <component :is="chip.icon" class="size-3" aria-hidden="true" />
            {{ chip.count }}
          </span>
        </TooltipTrigger>
        <TooltipContent>{{ chip.label }}</TooltipContent>
      </Tooltip>
      <span v-else :class="chipClass" :aria-label="chip.label">
        <!-- In quick-add mode the semantic icon and a "+" are both rendered; the group's
          hover/focus state (set on the enclosing trigger) shows exactly one of them. -->
        <template v-if="hoverAsAdd">
          <component
            :is="chip.icon"
            class="size-3 group-hover/add:hidden group-focus-within/add:hidden"
            aria-hidden="true"
          />
          <Plus
            class="hidden size-3 group-hover/add:block group-focus-within/add:block"
            aria-hidden="true"
          />
        </template>
        <component :is="chip.icon" v-else class="size-3" aria-hidden="true" />
        {{ chip.count }}
      </span>
    </template>
  </div>
</template>
