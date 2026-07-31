<script setup lang="ts">
import { computed, defineAsyncComponent, h } from 'vue'
import { Skeleton } from '@/components/ui/skeleton'
import { lifeSteps, type LifeLine } from '@/lib/lifeSeries'

// Every seat's life over the whole game, as one interactive step chart.
//
// Same interaction language as the collection value chart, over the same `ui/chart` primitives: a
// crosshair that snaps to the nearest change, a tooltip naming every seat's total there, and a
// legend that switches a seat's line off. The unovis body lives in LifeHistoryChartInner and loads
// lazily — the mat has to be tappable long before anyone scrolls down to the history, so unovis
// stays off the life route's critical chunk.
//
// **Change-spaced, not time-spaced.** Each recorded change gets the same width (the fold is
// `lifeSteps`); a break in the game is a break in play, not a hole in the chart. The clock rides
// along on each step and reads back out through the axis ticks and the tooltip heading.
const props = defineProps<{ lines: LifeLine[] }>()

const steps = computed(() => lifeSteps(props.lines))

// A chart of one column is a blank frame with a dot in it. That's only reachable when every
// recorded change belongs to a seat that has since been removed (the panel already gates on there
// being changes at all), so say so rather than draw it.
const hasHistory = computed(() => steps.value.length > 1)

const chartSkeleton = () => h(Skeleton, { class: 'h-48 w-full rounded-xl', 'aria-hidden': 'true' })
const LifeHistoryChartInner = defineAsyncComponent({
  loader: () => import('@/components/life/LifeHistoryChartInner.vue'),
  loadingComponent: chartSkeleton,
  delay: 0,
})
</script>

<template>
  <LifeHistoryChartInner v-if="hasHistory" :lines="lines" :steps="steps" />
  <p v-else class="text-muted-foreground py-6 text-sm">
    No changes left to chart — every one belongs to a player who has since been removed.
  </p>
</template>
