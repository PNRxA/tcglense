<script setup lang="ts">
import { Loader2 } from '@lucide/vue'
import CountLineCue from '@/components/cards/CountLineCue.vue'

// The shared "refetching over stale results" cue for the browse views' count lines: a small
// spinner + "Updating…", shown while keepPreviousData holds the previous page/filter results
// and the next set is in flight. A fragment (icon + label) so callers drop it straight into
// their count-line text; wrap the caller's own separators (e.g. "· ") around it as needed.
//
// `label` names the work for surfaces where "Updating…" is the wrong word — the deck panels'
// shuffles and recalculations (issue #596) read as "Dealing…"/"Recalculating…" — so a caller
// doesn't fork a second spinner-plus-text to change three glyphs. For a cue that isn't about
// work in flight, pass a different icon to `CountLineCue` rather than forking these metrics.
withDefaults(defineProps<{ label?: string }>(), { label: 'Updating…' })
</script>

<template>
  <CountLineCue :icon="Loader2" :label="label" spin />
</template>
