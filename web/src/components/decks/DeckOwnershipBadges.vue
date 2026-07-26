<script setup lang="ts">
import { Heart, Library } from '@lucide/vue'

// "You own N / you want N" chips for a card in the owner's deck view — how many copies the
// viewer's collection and wish list hold. Extracted so the image grid and the compact list
// (issue #570) show the same pair rather than two drifting copies of the markup.
//
// `overlay` is the only difference: on a card tile the pair is pinned to the top-right
// corner (the one the quantity control and legality chip never use), while in a list row it
// sits in normal flow. Each chip renders only when non-zero, and the whole block collapses
// when both are.
withDefaults(defineProps<{ owned: number; wanted: number; overlay?: boolean }>(), {
  overlay: false,
})
</script>

<template>
  <div
    v-if="owned > 0 || wanted > 0"
    class="flex shrink-0 items-center gap-1"
    :class="overlay ? 'absolute top-1.5 right-1.5 z-20' : ''"
  >
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
