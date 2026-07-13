<script setup lang="ts">
import { computed, type Component } from 'vue'
import { Heart, Layers, Plus, Sparkles } from '@lucide/vue'
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
    // The card's wish-list wanted count (regular + foil). When positive AND `kind` is
    // 'owned', a Heart chip is appended to the RIGHT of the total/foil chips, flagging that
    // the card is on the user's wish list (issue #364 follow-up). Like the total/foil chips,
    // it swaps its leading icon for a "+" under `hoverAsAdd`. Ignored when `kind` is 'wanted'
    // (the total chip is already a heart).
    wantedQuantity?: number
    // What the total/foil chips MEAN. 'owned' (default): the total chip leads with a
    // stacked-cards icon and reads "N total" (a collection holding). 'wanted': the total
    // chip leads with a Heart and reads "N wanted" — used by the product control and the
    // wishlist-targeting card grids, where the counts are wish-list wants, not owned copies.
    kind?: 'owned' | 'wanted'
    tooltip?: boolean
    hoverAsAdd?: boolean
  }>(),
  { wantedQuantity: 0, kind: 'owned', tooltip: true, hoverAsAdd: false },
)

const total = computed(() => props.quantity + props.foilQuantity)

// The chips to render, left to right: the total (when any), a foil chip (when some are
// foil), and — on an 'owned' badge that is also wish-listed — a Heart "wanted" chip. Each
// names its leading icon and its `aria-label`; under `hoverAsAdd` every chip swaps its
// leading icon for a "+".
const chips = computed(() => {
  const list: { key: string; icon: Component; count: number; label: string }[] = []
  if (total.value > 0) {
    list.push({
      key: 'total',
      icon: props.kind === 'wanted' ? Heart : Layers,
      count: total.value,
      label: props.kind === 'wanted' ? `${total.value} wanted` : `${total.value} total`,
    })
  }
  if (props.foilQuantity > 0) {
    list.push({
      key: 'foil',
      icon: Sparkles,
      count: props.foilQuantity,
      label: `${props.foilQuantity} foil`,
    })
  }
  if (props.kind === 'owned' && props.wantedQuantity > 0) {
    list.push({
      key: 'wanted',
      icon: Heart,
      count: props.wantedQuantity,
      label: `${props.wantedQuantity} wanted`,
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
