<script setup lang="ts">
import { Loader2 } from '@lucide/vue'

// The shared "refetching over stale results" cue for the browse views' count lines: a
// small spinner + "Updating…", shown while keepPreviousData holds the previous page/filter
// results and the next set is in flight. A fragment (icon + label) so callers drop it
// straight into their count-line text; wrap the caller's own separators (e.g. "· ")
// around it as needed.
</script>

<template><Loader2 class="mr-1 inline size-3.5 animate-spin align-[-0.15em]" />Updating…</template>
