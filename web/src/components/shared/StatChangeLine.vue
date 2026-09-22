<script setup lang="ts">
import { computed } from 'vue'
import { ChevronDown, Minus, TrendingDown, TrendingUp } from '@lucide/vue'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  CHANGE_WINDOW_OPTIONS,
  changeWindowLabel,
  changeWindowSentence,
  formatAsOfDate,
  isChangeWindow,
  type StatChange,
} from '@/lib/valueChange'
import type { MoverWindow } from '@/lib/api'

// The one-line movement under a summary stat: a trend glyph, the signed money change, the
// percentage chip and a muted window tag (`7D`, `1D`, `All`…) — the vocabulary the movers
// panel's selector uses. Gains take the success token, losses destructive, and an unchanged
// capture stays muted (a flat window is information, not good or bad news). The full sentence
// — what the window is and which capture it is measured to — is the line's accessible text:
// a visually-hidden span carries it while every visible fragment is `aria-hidden`, so a
// screen reader hears one sentence ("+$3.50, +2.8% over the last 7 days (as of Sep 22)")
// instead of a bare signed number; the same sentence is the mouse tooltip. (Not
// `aria-label`: a paragraph/div role prohibits an author name, so AT would drop it.)
//
// When the caller can change the window (`pickable`), the tag itself is the picker: a small
// button that opens a menu of the seven windows, each with its label and what it covers, and
// `pick` reports the choice — the landing binds every line's tag to one shared window, so
// picking on any of them moves them all. A line that can't be changed (a surface with no
// window state) renders the tag as plain text.
//
// The root is a `<div>` so it can sit inside the stat's `<dd>` — flow content is valid there,
// whereas a `<p>` beside the `<dd>` is not a legal child of a `<dl>`'s wrapper `<div>`.
const props = defineProps<{ change: StatChange; pickable?: boolean }>()
const emit = defineEmits<{ pick: [window: MoverWindow] }>()

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

// The menu's radio group hands back a string; only a known token moves the window.
function onPick(value: string | undefined) {
  if (isChangeWindow(value) && value !== props.change.window) emit('pick', value)
}
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
    <!-- The window tag: a menu trigger when the window can be changed, plain text otherwise. -->
    <DropdownMenu v-if="pickable">
      <DropdownMenuTrigger as-child>
        <button
          type="button"
          class="text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:ring-ring inline-flex items-center gap-0.5 rounded-md px-1 py-0.5 text-[0.65rem] font-semibold tracking-wide transition-colors focus-visible:ring-2 focus-visible:outline-none"
          :aria-label="`Change window: ${windowLabel}`"
        >
          {{ windowLabel }}
          <ChevronDown class="size-3" aria-hidden="true" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" class="w-56">
        <DropdownMenuLabel>Change over</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuRadioGroup :model-value="change.window" @update:model-value="onPick">
          <DropdownMenuRadioItem
            v-for="opt in CHANGE_WINDOW_OPTIONS"
            :key="opt.value"
            :value="opt.value"
          >
            <span class="w-8 font-semibold tabular-nums">{{ opt.label }}</span>
            <span class="text-muted-foreground">{{ opt.description }}</span>
          </DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
    <span
      v-else
      class="text-muted-foreground text-[0.65rem] font-semibold tracking-wide"
      aria-hidden="true"
    >
      {{ windowLabel }}
    </span>
  </div>
</template>
