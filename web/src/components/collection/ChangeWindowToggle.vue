<script setup lang="ts">
import { Button } from '@/components/ui/button'
import { CHANGE_WINDOW_OPTIONS } from '@/lib/valueChange'
import type { MoverWindow } from '@/lib/api'

// The segmented window picker the collection's price-movement surfaces share — the landing's
// value-change lines and the movers panel — so both offer the same seven windows with the
// same labels (1D / 7D / 30D / 1Y / 2Y / 3Y / All), in the same order, styled the same way.
// A plain `v-model` of the window token; the label names what the window scopes for AT.
const model = defineModel<MoverWindow>({ required: true })
defineProps<{ label: string }>()
</script>

<template>
  <div
    class="bg-muted/50 flex max-w-full flex-wrap items-center justify-end gap-1 rounded-lg p-0.5"
    role="group"
    :aria-label="label"
  >
    <Button
      v-for="opt in CHANGE_WINDOW_OPTIONS"
      :key="opt.value"
      type="button"
      :variant="model === opt.value ? 'secondary' : 'ghost'"
      size="sm"
      class="h-8 px-2.5 text-xs font-medium"
      :aria-pressed="model === opt.value"
      @click="model = opt.value"
    >
      {{ opt.label }}
    </Button>
  </div>
</template>
