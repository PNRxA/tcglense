<script setup lang="ts">
import { computed } from 'vue'
import { useCurrency } from '@/composables/useCurrency'
import type { BreakdownBucket } from '@/lib/api'
import { breakdownBars, type BreakdownFacet } from '@/lib/holdingBreakdown'
import type { CountNoun } from '@/lib/ownership'

// One facet of the holdings breakdown (issue #680) as labelled rows: a bar per bucket
// whose length is the bucket's share of the facet's priced value, with the value and the
// copy count beside it. The deck composition bars (`DeckStatBars`) draw *counts*; these
// draw *money*, which is why the scale here is the facet's summed value rather than its
// tallest bucket — a facet whose value is spread evenly must read as such, and a bar's
// length must mean the same thing whichever facet the switch is on.
const props = defineProps<{
  facet: BreakdownFacet
  buckets: BreakdownBucket[]
  /** The copy noun — `owned` for the collection, `wanted` for the wish list. */
  countNoun: CountNoun
}>()
const money = useCurrency()

const bars = computed(() => breakdownBars(props.facet, props.buckets))

/** "Rare: $30.00 · 2 owned" — one wording for a bar's accessible label. */
function barLabel(bar: (typeof bars.value)[number]): string {
  const value = money.formatUsd(bar.valueUsd) ?? 'unpriced'
  return `${bar.label}: ${value} · ${bar.copies.toLocaleString()} ${props.countNoun}`
}
</script>

<template>
  <ul class="space-y-2.5">
    <li v-for="bar in bars" :key="bar.key">
      <div class="mb-1 flex items-baseline justify-between gap-3 text-xs">
        <span class="min-w-0 truncate font-medium">{{ bar.label }}</span>
        <span class="text-muted-foreground shrink-0 tabular-nums">
          <span v-if="bar.valueUsd" class="text-foreground font-semibold">
            {{ money.formatUsd(bar.valueUsd) }}
          </span>
          <span v-else>Unpriced</span>
          <span aria-hidden="true"> · </span>
          <span>{{ bar.copies.toLocaleString() }} {{ countNoun }}</span>
        </span>
      </div>
      <div class="bg-muted h-2 overflow-hidden rounded-full">
        <div
          class="h-full rounded-full transition-[width]"
          :class="bar.share > 0 ? 'min-w-0.5' : ''"
          :style="{ width: `${bar.share * 100}%`, backgroundColor: bar.color }"
          role="img"
          :aria-label="barLabel(bar)"
        />
      </div>
    </li>
  </ul>
</template>
