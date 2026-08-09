<script setup lang="ts">
import { TriangleAlert } from '@lucide/vue'
import CountLineCue from '@/components/cards/CountLineCue.vue'

// The non-destructive counterpart to an error paragraph, for the state query-core calls
// `isRefetchError`: a *background* refetch failed while its cached content is still on screen.
// Show this above the content and leave the content alone, rather than replacing a perfectly
// good list with "couldn't load" the first time a focus refetch hiccups (issue #622).
//
// It pairs with gating the destructive branch on `isLoadingError` — a failure with nothing ever
// loaded, which is the only case where there is genuinely nothing to show. Gating that branch on
// bare `isError` is the bug: query-core's error reducer flips `status` to 'error' on ANY failed
// fetch while KEEPING `data`.
//
// `label` names what went stale, since "your alerts" and "your decks" read better than a generic
// line on a page that shows one specific thing.
withDefaults(defineProps<{ label?: string }>(), {
  label: "Couldn't refresh — showing the last loaded results.",
})
</script>

<template>
  <p class="text-destructive mb-3 text-sm" aria-live="polite">
    <CountLineCue :icon="TriangleAlert" :label="label" />
  </p>
</template>
