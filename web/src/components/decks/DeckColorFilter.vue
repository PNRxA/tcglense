<script setup lang="ts">
import { Toggle } from '@/components/ui/toggle'
import { DECK_FILTER_COLOR_OPTIONS, type DeckFilterColor } from '@/lib/deckFilter'

// Multi-select colour pips for the deck card filter, drawn with the bundled mana-font
// glyphs. An empty selection means "no colour constraint"; the match semantics live in
// lib/deckFilter — this is only the control.
const model = defineModel<DeckFilterColor[]>({ required: true })

function toggle(color: DeckFilterColor) {
  model.value = model.value.includes(color)
    ? model.value.filter((selected) => selected !== color)
    : [...model.value, color]
}

// The advanced-search panel's pip styling: a Toggle whose "on" state is a ring, not the
// default filled background (which would clash with the coloured mana glyph).
const pipClass =
  'size-8 min-w-0 rounded-full p-0 opacity-50 hover:bg-transparent hover:opacity-100 data-[state=on]:bg-transparent data-[state=on]:opacity-100 data-[state=on]:ring-2 data-[state=on]:ring-ring data-[state=on]:ring-offset-1 data-[state=on]:ring-offset-background'
</script>

<template>
  <div role="group" aria-label="Filter cards by colour" class="flex items-center gap-1.5">
    <Toggle
      v-for="option in DECK_FILTER_COLOR_OPTIONS"
      :key="option.value"
      :model-value="model.includes(option.value)"
      :aria-label="`Filter to ${option.label.toLowerCase()}`"
      :title="option.label"
      :class="pipClass"
      @update:model-value="toggle(option.value)"
    >
      <i class="ms ms-cost" :class="option.icon" aria-hidden="true" />
    </Toggle>
  </div>
</template>
