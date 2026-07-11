<script setup lang="ts">
import { cn } from '@/lib/utils'

// The GroupViewToggle-style segmented control both holding landings share: pick between
// the held sets (Collected / Wishlisted) and the whole catalog ("All sets"). A thin
// presentational wrapper over the boolean `showAllSets` v-model — the parent's setter runs
// the same `router.replace`, keeping the scope in the URL.
const showAllSets = defineModel<boolean>({ required: true })
defineProps<{ collectedLabel: string }>()
</script>

<template>
  <div class="bg-muted text-muted-foreground inline-flex shrink-0 rounded-md p-0.5 text-sm">
    <button
      type="button"
      :class="
        cn(
          'rounded px-3 py-1.5 font-medium transition-colors',
          !showAllSets ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
        )
      "
      @click="showAllSets = false"
    >
      {{ collectedLabel }}
    </button>
    <button
      type="button"
      :class="
        cn(
          'rounded px-3 py-1.5 font-medium transition-colors',
          showAllSets ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
        )
      "
      @click="showAllSets = true"
    >
      All sets
    </button>
  </div>
</template>
