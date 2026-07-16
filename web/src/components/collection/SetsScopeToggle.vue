<script setup lang="ts">
import { cn } from '@/lib/utils'

// The GroupViewToggle-style two-option segmented control used by holding landings and the
// movers panel. The first label is caller-defined; the second defaults to "All sets" but can
// be overridden (Singles / Sealed). The parent owns what the boolean means.
const showSecond = defineModel<boolean>({ required: true })
withDefaults(defineProps<{ collectedLabel: string; secondLabel?: string }>(), {
  secondLabel: 'All sets',
})
</script>

<template>
  <div class="bg-muted text-muted-foreground inline-flex shrink-0 rounded-md p-0.5 text-sm">
    <button
      type="button"
      :class="
        cn(
          'rounded px-3 py-1.5 font-medium transition-colors',
          !showSecond ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
        )
      "
      @click="showSecond = false"
    >
      {{ collectedLabel }}
    </button>
    <button
      type="button"
      :class="
        cn(
          'rounded px-3 py-1.5 font-medium transition-colors',
          showSecond ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
        )
      "
      @click="showSecond = true"
    >
      {{ secondLabel }}
    </button>
  </div>
</template>
