<script setup lang="ts">
import { computed } from 'vue'
import type { ImportProgress } from '@/lib/api'

// A live progress readout for a running collection import: a labelled bar fed by the
// job's `progress` (rows fetched / total). When the provider reported a total up front
// (a full import) the bar is determinate with a percentage; a smart sync stops early and
// reports no total, so it shows the running fetched count over an indeterminate bar.
// Shared by the import dialog and the collection landing's re-sync controls.
const props = defineProps<{ progress: ImportProgress }>()

const fetched = computed(() => props.progress.fetched)
const total = computed(() => props.progress.total ?? null)
const pct = computed(() => {
  const t = total.value
  if (t == null || t <= 0) return null
  // Clamp: a concurrent upstream edit could push fetched past the first-page total.
  return Math.min(100, Math.round((fetched.value / t) * 100))
})
const label = computed(() =>
  total.value != null
    ? `Fetched ${fetched.value.toLocaleString()} of ${total.value.toLocaleString()} cards`
    : `Fetched ${fetched.value.toLocaleString()} cards…`,
)
</script>

<template>
  <div class="space-y-1" aria-live="polite">
    <div class="text-muted-foreground flex items-center justify-between text-xs">
      <span>{{ label }}</span>
      <span v-if="pct != null">{{ pct }}%</span>
    </div>
    <div class="bg-muted h-1.5 w-full overflow-hidden rounded-full">
      <!-- Determinate: a total is known, so fill to the fetched fraction. -->
      <div
        v-if="pct != null"
        class="bg-primary h-full rounded-full transition-[width] duration-500"
        :style="{ width: `${pct}%` }"
        role="progressbar"
        :aria-valuenow="pct"
        :aria-valuemin="0"
        :aria-valuemax="100"
      />
      <!-- Indeterminate (smart sync): no total to fill to; the growing count conveys motion. -->
      <div
        v-else
        class="bg-primary/70 h-full w-1/3 animate-pulse rounded-full"
        role="progressbar"
      />
    </div>
  </div>
</template>
