<script setup lang="ts">
// Interactive legend for the price/value chart: one button per plotted series that toggles
// that line's visibility. Presentational — it owns no state, it just renders the items it's
// handed and emits the series key on click. The chart body (PriceChartInner) keeps the
// visibility state, because it's what gates the VisLine components, and it enforces the
// "at least one line stays shown" rule; here `visible` is already the reconciled truth.
interface LegendItem {
  /** Stable series key (e.g. `usd`), echoed back on toggle. */
  key: string
  /** Human label shown next to the swatch. */
  label: string
  /** Swatch colour — a chart CSS token, so it follows the theme. */
  color: string
  /** Whether this series is currently drawn. */
  visible: boolean
}

defineProps<{ items: LegendItem[] }>()
const emit = defineEmits<{ toggle: [key: string] }>()
</script>

<template>
  <div class="mt-3 flex flex-wrap items-center justify-center gap-2">
    <button
      v-for="item in items"
      :key="item.key"
      type="button"
      class="focus-visible:ring-ring/50 flex items-center gap-1.5 rounded-md px-1.5 py-0.5 text-xs font-medium transition-opacity focus-visible:ring-2 focus-visible:outline-none"
      :class="item.visible ? 'text-foreground' : 'text-muted-foreground'"
      :aria-pressed="item.visible"
      :aria-label="`${item.visible ? 'Hide' : 'Show'} ${item.label}`"
      @click="emit('toggle', item.key)"
    >
      <span
        class="h-2 w-2 shrink-0 rounded-xs"
        :class="{ 'opacity-30': !item.visible }"
        :style="{ backgroundColor: item.color }"
      />
      <span :class="{ 'line-through': !item.visible }">{{ item.label }}</span>
    </button>
  </div>
</template>
