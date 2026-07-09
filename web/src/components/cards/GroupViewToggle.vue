<script setup lang="ts">
import { cn } from '@/lib/utils'

// The "<grouped> / All cards" segmented control for a set that can be browsed grouped —
// by Secret Lair drop or by card sub-type (treatment). Shared by the catalog set view and
// the collection/wish-list browse views. The caller owns the visibility guard (`hasDrops`
// / `hasSubtypes`) and the `select` handler (which restarts paging / keeps its own view
// state); this is just the two-button presentation. `label` is the grouped button's text
// ("By drop" or "By treatment"), sourced from the grouping's `groupLabel`.
defineProps<{ grouped: boolean; label: string }>()
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
      All cards
    </button>
  </div>
</template>
