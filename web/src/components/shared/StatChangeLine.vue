<script setup lang="ts">
import { computed } from 'vue'
import { Minus, TrendingDown, TrendingUp } from '@lucide/vue'
import {
  changeWindowLabel,
  changeWindowSentence,
  formatAsOfDate,
  type StatChange,
} from '@/lib/valueChange'

// The one-line movement under a summary stat: a trend glyph, the signed money change, the
// percentage chip and a muted window tag (`7D`, `1D`, `All`…) — the vocabulary the shared
// window picker and the movers panel use. Gains take the success token, losses destructive,
// and an unchanged capture stays muted (a flat window is information, not good or bad news).
// The full sentence — what the window is and which capture it is measured to — is the line's
// accessible text: a visually-hidden span carries it while every visible fragment is
// `aria-hidden`, so a screen reader hears one sentence ("+$3.50, +2.8% over the last 7 days
// (as of Sep 22)") instead of a bare signed number; the same sentence is the mouse tooltip.
// (Not `aria-label`: a paragraph/div role prohibits an author name, so AT would drop it.)
// The root is a `<div>` so it can sit inside the stat's `<dd>` — flow content is valid there,
// whereas a `<p>` beside the `<dd>` is not a legal child of a `<dl>`'s wrapper `<div>`.
const props = defineProps<{ change: StatChange }>()

const toneClass = computed(() => {
  switch (props.change.direction) {
    case 'up':
      return 'text-success'
    case 'down':
      return 'text-destructive'
    default:
      return 'text-muted-foreground'
  }
})
const chipClass = computed(() => {
  switch (props.change.direction) {
    case 'up':
      return 'bg-success/15 text-success'
    case 'down':
      return 'bg-destructive/15 text-destructive'
    default:
      return 'bg-muted text-muted-foreground'
  }
})
const Icon = computed(() => {
  switch (props.change.direction) {
    case 'up':
      return TrendingUp
    case 'down':
      return TrendingDown
    default:
      return Minus
  }
})
const windowLabel = computed(() => changeWindowLabel(props.change.window))
const description = computed(() => {
  const asOf = props.change.asOf ? ` (as of ${formatAsOfDate(props.change.asOf)})` : ''
  const pct = props.change.pctText ? `, ${props.change.pctText}` : ''
  return `${props.change.text}${pct} ${changeWindowSentence(props.change.window)}${asOf}`
})
</script>

<template>
  <div
    class="stat-change mt-0.5 flex items-center gap-1.5 text-xs font-medium tabular-nums"
    :class="toneClass"
    :title="description"
  >
    <span class="sr-only">{{ description }}</span>
    <component :is="Icon" class="size-3.5 shrink-0" aria-hidden="true" />
    <span aria-hidden="true">{{ change.text }}</span>
    <span
      v-if="change.pctText"
      class="rounded-md px-1.5 py-0.5 text-[0.65rem] leading-none font-semibold"
      :class="chipClass"
      aria-hidden="true"
    >
      {{ change.pctText }}
    </span>
    <span
      class="text-muted-foreground text-[0.65rem] font-semibold tracking-wide"
      aria-hidden="true"
    >
      {{ windowLabel }}
    </span>
  </div>
</template>
