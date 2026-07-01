<script setup lang="ts">
import { cn } from '@/lib/utils'

// The "By drop / All cards" segmented control for a drop-grouped set, shared by the
// catalog set view and the collection browse view. The caller owns the visibility guard
// (`hasDrops`, optionally scoped) and the `select` handler (which restarts paging / keeps
// its own view state); this is just the two-button presentation.
defineProps<{ byDrop: boolean }>()
const emit = defineEmits<{ select: ['drops' | 'all'] }>()
</script>

<template>
  <div class="bg-muted text-muted-foreground inline-flex rounded-md p-0.5 text-sm">
    <button
      type="button"
      :class="
        cn(
          'rounded px-3 py-1.5 font-medium transition-colors',
          byDrop ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
        )
      "
      @click="emit('select', 'drops')"
    >
      By drop
    </button>
    <button
      type="button"
      :class="
        cn(
          'rounded px-3 py-1.5 font-medium transition-colors',
          !byDrop ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
        )
      "
      @click="emit('select', 'all')"
    >
      All cards
    </button>
  </div>
</template>
