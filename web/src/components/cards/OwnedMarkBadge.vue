<script setup lang="ts">
import { Check } from '@lucide/vue'

// A small "Owned" marker overlaid on a card in the wish-list browse grids when the ghost
// button's "Show owned (in collection)" setting is on (issue #213): it flags a card the
// viewer already owns in their *collection* while they browse their wish list, so a card
// they've bought stands out from the ones still to buy. Anchored top-left, clear of the
// quick-add control's bottom-left corner and the card name below. Passive (not a link or
// button), so a native `title` + `aria-label` carry the meaning without a tooltip provider.
// `emerald-700` (not -600) keeps white-on-fill at WCAG AA (≥ 4.5:1) for the small label.
</script>

<template>
  <span
    class="absolute top-1.5 left-1.5 z-20 inline-flex items-center gap-0.5 rounded-md bg-emerald-700 px-1.5 py-0.5 text-xs font-semibold text-white shadow"
    title="In your collection"
    aria-label="Owned in your collection"
  >
    <Check class="size-3" aria-hidden="true" />
    Owned
  </span>
</template>
