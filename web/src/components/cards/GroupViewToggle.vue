<script setup lang="ts">
import { cn } from '@/lib/utils'

// The "<grouped> / <all>" segmented control for a listing that can be browsed grouped — by
// Secret Lair drop, by card sub-type (treatment), or by set (the precon browse). Shared by the
// catalog set view, the collection/wish-list browse views and the precon browse. The caller
// owns the visibility guard (`hasDrops` / `hasSubtypes`) and the `select` handler (which
// restarts paging / keeps its own view state); this is just the two-button presentation.
//
// `label` is the grouped button's text ("By drop" / "By treatment" / "By set"), sourced from
// the grouping's `groupLabel`. `allLabel` is the ungrouped button's — "All cards" for every
// card listing (the default, so those callers are unchanged), "All decks" for precons.
withDefaults(defineProps<{ grouped: boolean; label: string; allLabel?: string }>(), {
  allLabel: 'All cards',
})
const emit = defineEmits<{ select: ['grouped' | 'all'] }>()
</script>

<template>
  <div class="bg-muted text-muted-foreground inline-flex rounded-md p-0.5 text-sm">
    <button
      type="button"
      :class="
        cn(
          'rounded px-3 py-1.5 font-medium transition-colors',
          grouped ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
        )
      "
      @click="emit('select', 'grouped')"
    >
      {{ label }}
    </button>
    <button
      type="button"
      :class="
        cn(
          'rounded px-3 py-1.5 font-medium transition-colors',
          !grouped ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
        )
      "
      @click="emit('select', 'all')"
    >
      {{ allLabel }}
    </button>
  </div>
</template>
