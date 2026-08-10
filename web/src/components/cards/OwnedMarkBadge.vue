<script setup lang="ts">
import { Check } from '@lucide/vue'

// A small "Owned" marker overlaid on a card in the wish-list browse grids when the ghost
// button's "Show owned (in collection)" setting is on (issue #213): it flags a card the
// viewer already owns in their *collection* while they browse their wish list, so a card
// they've bought stands out from the ones still to buy. Anchored top-left, clear of the
// quick-add control's bottom-left corner and the card name below. Passive (not a link or
// button), so a native `title` + `aria-label` carry the meaning without a tooltip provider.
// The `success`/`success-foreground` token pair is AA-checked in docs/design-system.md,
// so the small label stays readable on the fill.
</script>

<template>
  <span
    class="bg-success text-success-foreground absolute top-1.5 left-1.5 z-20 inline-flex items-center gap-0.5 rounded-md px-1.5 py-0.5 text-xs font-semibold shadow"
    title="In your collection"
    aria-label="Owned in your collection"
  >
    <Check class="size-3" aria-hidden="true" />
    Owned
  </span>
</template>
