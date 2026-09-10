<script setup lang="ts">
import { Boxes, Copy, Library } from '@lucide/vue'

// Presentational-only mock of the preconstructed-deck browser, rendered inside the homepage's
// decorative feature-demo panels. Purely static illustration — no props, no data flow, and
// nothing interactive or focusable.
const decks = [
  { set: 'MH3', price: '$48.30', nameWidth: 'w-full', colors: [1, 2] },
  { set: 'LCC', price: '$112.75', nameWidth: 'w-5/6', colors: [3, 4, 5] },
  { set: 'OTC', price: '$39.90', nameWidth: 'w-3/4', colors: [5] },
]

// The five colour-pip slots; a deck's own colours tint theirs with the chart tokens.
const pips = [1, 2, 3, 4, 5]
</script>

<template>
  <!-- Decorative mock UI — illustrative values, not real market data. -->
  <div class="flex items-center justify-between gap-2">
    <span class="flex items-center gap-1.5 text-sm font-semibold">
      <Boxes class="size-4" aria-hidden="true" />
      Preconstructed decks
    </span>
    <div class="bg-muted/50 inline-flex items-center gap-1 rounded-lg p-0.5">
      <span class="bg-background text-foreground rounded px-2 py-1 text-xs font-medium">
        By set
      </span>
      <span class="text-muted-foreground rounded px-2 py-1 text-xs font-medium">By type</span>
      <span class="text-muted-foreground rounded px-2 py-1 text-xs font-medium">Price</span>
    </div>
  </div>

  <div class="mt-4 grid grid-cols-3 gap-3">
    <div v-for="deck in decks" :key="deck.set" class="bg-muted overflow-hidden rounded-lg border">
      <!-- Face-card art area. -->
      <div class="from-primary/25 via-primary/10 h-12 bg-gradient-to-br to-transparent"></div>
      <div class="space-y-2 p-2">
        <div class="bg-foreground/15 h-1.5 rounded-full" :class="deck.nameWidth"></div>
        <span class="text-muted-foreground inline-block rounded border px-1.5 text-[10px]">
          {{ deck.set }}
        </span>
        <div class="flex items-center gap-1.5">
          <span class="flex items-center gap-1">
            <span
              v-for="pip in pips"
              :key="pip"
              class="size-2 rounded-full"
              :class="deck.colors.includes(pip) ? '' : 'bg-foreground/10'"
              :style="deck.colors.includes(pip) ? `background: var(--chart-${pip})` : undefined"
            ></span>
          </span>
          <span class="ml-auto text-xs tabular-nums">{{ deck.price }}</span>
        </div>
      </div>
    </div>
  </div>

  <div class="mt-4 flex flex-wrap items-center gap-1.5 border-t pt-3">
    <span
      class="border-primary/30 bg-primary/10 text-primary inline-flex items-center gap-1 rounded-full border px-2.5 py-0.5 text-xs font-medium"
    >
      <Copy class="size-3" aria-hidden="true" />
      Copy to your decks
    </span>
    <span
      class="text-muted-foreground inline-flex items-center gap-1 rounded-full border px-2.5 py-0.5 text-xs"
    >
      <Library class="size-3" aria-hidden="true" />
      Add to collection
    </span>
  </div>
</template>
