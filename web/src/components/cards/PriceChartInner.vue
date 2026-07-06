<script setup lang="ts">
import { computed } from 'vue'
import { VisAxis, VisLine, VisScatter, VisXYContainer } from '@unovis/vue'
import {
  type ChartConfig,
  ChartContainer,
  ChartCrosshair,
  ChartTooltip,
  ChartTooltipContent,
  componentToString,
} from '@/components/ui/chart'
import { type PriceRange } from '@/lib/api'

// The unovis-backed chart body, split out of PriceChart so unovis stays off every detail
// route's critical chunk (this loads via defineAsyncComponent, in parallel with the
// wrapper's price-history query). The wrapper owns the query, range state + buttons, and
// the pending/error/empty branches; this just plots the series it's handed. The two USD
// fields are all it reads, so any series carrying them satisfies it.
interface PricePointLike {
  date: string
  usd: string | null
  usd_foil: string | null
}
const props = defineProps<{ series: PricePointLike[]; range: PriceRange }>()

// One plotted day. Dates become epoch ms for a continuous x-scale; price strings become
// numbers, with null kept as null so the line *gaps* over missing days rather than
// dropping to zero.
interface PricePlot {
  date: number
  usd: number | null
  usdFoil: number | null
}

function toNumber(value: string | null): number | null {
  if (value == null) return null
  const n = Number.parseFloat(value)
  return Number.isFinite(n) ? n : null
}

const points = computed<PricePlot[]>(() =>
  props.series.map((p) => ({
    date: new Date(p.date).getTime(),
    usd: toNumber(p.usd),
    usdFoil: toNumber(p.usd_foil),
  })),
)

// No point markers: the crosshair snaps to the nearest datum on hover, so the dots are
// redundant. The one exception is a single-datum series, which has no line stroke to
// draw — without a dot it'd render nothing at all.
const showDots = computed(() => points.value.length === 1)

// Series legend/tooltip metadata. Colours are the theme's chart tokens, which the CSS
// variables resolve differently in light vs dark, so the chart follows the theme.
const chartConfig = {
  usd: { label: 'USD', color: 'var(--chart-1)' },
  usdFoil: { label: 'USD foil', color: 'var(--chart-2)' },
} satisfies ChartConfig

const x = (d: PricePlot) => d.date
const usdY = (d: PricePlot) => d.usd
const foilY = (d: PricePlot) => d.usdFoil

// Full date for the tooltip (built once below, so it must stay stable).
const dateFmt = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
  year: 'numeric',
})
const formatDate = (tick: number | Date) =>
  dateFmt.format(typeof tick === 'number' ? new Date(tick) : tick)

// Axis ticks: short windows read better as "Jul 1", multi-month/-year windows as
// "Jul 2026". A computed-returning-function so the axis re-renders on range change.
const axisDateShort = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' })
const axisDateLong = new Intl.DateTimeFormat(undefined, { month: 'short', year: 'numeric' })
const formatAxisDate = computed(() => {
  const fmt = props.range === '7d' || props.range === '30d' ? axisDateShort : axisDateLong
  return (tick: number | Date) => fmt.format(typeof tick === 'number' ? new Date(tick) : tick)
})

const formatPrice = (tick: number | Date) =>
  `$${Number(tick).toLocaleString(undefined, { maximumFractionDigits: 2 })}`

// Rich tooltip rendered from the shadcn primitive (built once during setup).
const tooltipTemplate = componentToString(chartConfig, ChartTooltipContent, {
  labelFormatter: formatDate,
  indicator: 'line',
})
</script>

<template>
  <ChartContainer :config="chartConfig" class="aspect-auto h-64 w-full" :cursor="true">
    <VisXYContainer :data="points" :margin="{ left: 8, right: 8 }">
      <VisLine :x="x" :y="usdY" color="var(--chart-1)" :line-width="2" />
      <VisLine :x="x" :y="foilY" color="var(--chart-2)" :line-width="2" />
      <VisScatter v-if="showDots" :x="x" :y="usdY" color="var(--chart-1)" :size="36" />
      <VisScatter v-if="showDots" :x="x" :y="foilY" color="var(--chart-2)" :size="36" />
      <VisAxis
        type="x"
        :x="x"
        :tick-format="formatAxisDate"
        :num-ticks="5"
        :grid-line="false"
        :tick-line="false"
        :domain-line="false"
      />
      <VisAxis
        type="y"
        :tick-format="formatPrice"
        :grid-line="true"
        :tick-line="false"
        :domain-line="false"
      />
      <ChartCrosshair color="var(--chart-1)" :template="tooltipTemplate" />
      <ChartTooltip />
    </VisXYContainer>
  </ChartContainer>
</template>
