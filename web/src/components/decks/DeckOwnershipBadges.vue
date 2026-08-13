<script setup lang="ts">
import { Heart, Library } from '@lucide/vue'

// "You own N / you want N" chips for a card in the owner's deck view — how many copies the
// viewer's collection and wish list hold. Extracted so the image grid and the compact list
// (issue #570) show the same pair rather than two drifting copies of the markup.
//
// Layout-free, and identical on both surfaces: the list row gives it a column, the image grid
// stacks it above the deck count in `DeckTileBadges`' bottom-left column. It used to carry an
// `overlay` flag that pinned it to a tile's top-right corner instead — the one place a chip
// covers art nobody has already given up (the count and the breach chip share the bottom
// edge), and the far corner from the count it qualifies. Each chip renders only when
// non-zero, and the whole block collapses when both are — which is what keeps the grid's
// column from opening a gap for a card you neither own nor want.
defineProps<{ owned: number; wanted: number }>()
</script>

<template>
  <div v-if="owned > 0 || wanted > 0" class="flex shrink-0 items-center gap-1">
    <span
      v-if="owned > 0"
      class="bg-background/90 text-foreground inline-flex cursor-default items-center gap-0.5 rounded-md border px-1.5 py-0.5 text-xs shadow select-none"
      :title="`You own ${owned} of this card`"
    >
      <Library class="size-3" aria-hidden="true" />{{ owned }}
    </span>
    <span
      v-if="wanted > 0"
      class="bg-background/90 text-foreground inline-flex cursor-default items-center gap-0.5 rounded-md border px-1.5 py-0.5 text-xs shadow select-none"
      :title="`You have ${wanted} of this card on your wish list`"
    >
      <Heart class="size-3" aria-hidden="true" />{{ wanted }}
    </span>
  </div>
</template>
