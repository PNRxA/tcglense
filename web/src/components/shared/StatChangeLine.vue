<script setup lang="ts">
import { computed } from 'vue'
import { Minus, TrendingDown, TrendingUp } from '@lucide/vue'
import { formatAsOfDate, type StatChange } from '@/lib/valueChange'

// The one-line daily movement under a summary stat: a trend glyph, the signed money change,
// the percentage chip and a muted `1D` window tag — the vocabulary the movers panel's window
// selector already uses. Gains take the success token, losses destructive, and an unchanged
// capture stays muted (a flat day is information, not good or bad news). The full sentence
// — what the window is and which capture it is measured to — rides the title/aria-label so
// the row itself stays scannable.
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
const description = computed(() => {
  const asOf = props.change.asOf ? ` (as of ${formatAsOfDate(props.change.asOf)})` : ''
  const pct = props.change.pctText ? `, ${props.change.pctText}` : ''
  return `${props.change.text}${pct} since the previous day's captured prices${asOf}`
})
</script>

<template>
  <p
    class="mt-0.5 flex items-center gap-1.5 text-xs font-medium tabular-nums"
    :class="toneClass"
    :title="description"
    :aria-label="description"
  >
    <component :is="Icon" class="size-3.5 shrink-0" aria-hidden="true" />
    <span>{{ change.text }}</span>
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
      1D
    </span>
  </p>
</template>
