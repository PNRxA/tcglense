<script setup lang="ts">
import { Loader2 } from '@lucide/vue'

// Wraps a results grid so a page/filter change held by keepPreviousData reads as a content
// update rather than a frozen page: the stale grid dims and a spinner floats near its top —
// where a page change lands the viewport (issue #258 scrolls the results top into view). Pairs
// with the count line's UpdatingCue and the pager's button spinner (issue #223); this is the
// piece that was missing for the grid itself under high latency (issue #264). The stale page is
// kept underneath (keepPreviousData) rather than swapped for a skeleton, so nothing reflows, and
// it's marked `inert` while loading so a stray click/keypress can't hit a card about to change
// (and assistive tech skips the mid-update content — the wrapper's aria-busy carries the state).
defineProps<{ loading?: boolean }>()
</script>

<template>
  <div class="relative" :aria-busy="loading || undefined">
    <div
      class="transition-opacity duration-200"
      :class="{ 'pointer-events-none select-none opacity-50': loading }"
      :inert="loading || undefined"
    >
      <slot />
    </div>
    <div
      v-if="loading"
      class="pointer-events-none absolute inset-0 flex items-start justify-center"
    >
      <Loader2 class="text-muted-foreground mt-16 size-8 animate-spin" />
    </div>
  </div>
</template>
