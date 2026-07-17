<script setup lang="ts">
import { computed } from 'vue'
import { VisAxis, VisLine, VisScatter, VisXYContainer } from '@unovis/vue'
import {
  type ChartConfig,
  ChartContainer,
  ChartCrosshair,
  ChartSeriesLegend,
  ChartTooltip,
  ChartTooltipContent,
  componentToString,
} from '@/components/ui/chart'
import { useSeriesToggle } from '@/composables/useSeriesToggle'
import { type PriceRange } from '@/lib/api'
import type { SupportedCurrency } from '@/lib/currency'

// The unovis-backed chart body, split out of PriceChart so unovis stays off every detail
// route's critical chunk (this loads via defineAsyncComponent, in parallel with the
// wrapper's price-history query). The wrapper owns the query, range state + buttons, and
// the pending/error/empty branches; this plots the series it's handed and (when `toggleable`)
// owns the per-line show/hide state behind the legend. The two USD fields are all it reads,
// so any series carrying them satisfies it.
interface PricePointLike {
  date: string
  usd: string | null
  usd_foil: string | null
}
const props = defineProps<{
  series: PricePointLike[]
  range: PriceRange
  currency: SupportedCurrency
  /** Plot only the USD line (no foil series). Stable per chart instance, so the
   * once-built tooltip stays consistent with it. */
  singleSeries?: boolean
  /** Semantic names for the two generic price fields when they are not regular/foil. */
  seriesLabels?: { primary: string; secondary: string }
  /** Show a clickable legend that toggles each plotted line on/off. Opt-in (the collection
   * value chart uses it); off for the card/product detail charts, which stay legend-less. */
  toggleable?: boolean
}>()

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
// redundant. The exception is a series with a single *plotted* point, which has no line
// stroke to draw — a one-row series, or a collection line whose holdings only have a captured
// price on its newest day (older days gap over their nulls). Without a dot it'd render nothing
// at all. Null-gapping keeps the non-null run contiguous, so two or more points always stroke
// a line.
const plottedUsd = computed(() => points.value.filter((p) => p.usd != null).length)
const plottedFoil = computed(() => points.value.filter((p) => p.usdFoil != null).length)
const showUsdDot = computed(() => points.value.length === 1 || plottedUsd.value === 1)
const showFoilDot = computed(() => points.value.length === 1 || plottedFoil.value === 1)

// Series legend/tooltip metadata. Colours are the theme's chart tokens, which the CSS
// variables resolve differently in light vs dark, so the chart follows the theme. In
// single-series mode the foil entry is dropped so the tooltip has no
// empty "USD foil" row. Keyed off the stable `singleSeries` prop (never the data), so the
// once-built tooltip template below can't desync from the plotted lines across navigation.
const chartConfig = computed<ChartConfig>(() => {
  const config: ChartConfig = {
    usd: {
      label: props.seriesLabels?.primary ?? props.currency,
      color: 'var(--chart-1)',
    },
  }
  if (!props.singleSeries) {
    config.usdFoil = {
      label: props.seriesLabels?.secondary ?? `${props.currency} foil`,
      color: 'var(--chart-2)',
    }
  }
  return config
})

// --- Per-line visibility (opt-in via `toggleable`) ---------------------------------------
// Only series that actually have a plotted point are togglable: a collection with no sealed
// products has an all-null secondary line, and offering a switch for a line that isn't there
// would be noise. So the legend appears only once there are two real lines to choose between;
// with one (or none) there's nothing to toggle and the chart stays legend-less as before.
// The show/hide engine (which line stays drawn, the keep-one-line rule) lives in the
// useSeriesToggle composable so it can be unit-tested without the unovis chart body.
const dataKeys = computed<('usd' | 'usdFoil')[]>(() => {
  const keys: ('usd' | 'usdFoil')[] = []
  if (plottedUsd.value > 0) keys.push('usd')
  if (!props.singleSeries && plottedFoil.value > 0) keys.push('usdFoil')
  return keys
})
const { shownKeys, canToggle, isShown, toggle: toggleSeries } = useSeriesToggle(dataKeys)

// The toggle engine only governs the opt-in path; a plain chart keeps its original gating so
// its lines and tooltip are byte-for-byte as before.
const shownUsd = computed(() => (props.toggleable ? isShown('usd') : true))
const shownFoil = computed(() => (props.toggleable ? isShown('usdFoil') : !props.singleSeries))

const showLegend = computed(() => props.toggleable && canToggle.value)
const legendItems = computed(() =>
  dataKeys.value.map((key) => ({
    key,
    label: String(chartConfig.value[key]?.label ?? key),
    color: key === 'usd' ? 'var(--chart-1)' : 'var(--chart-2)',
    visible: isShown(key),
  })),
)

// Tooltip mirrors the drawn lines: a hidden series drops its row instead of showing a stale
// value. An untoggled chart keeps the full config, so its tooltip is unchanged.
const visibleConfig = computed<ChartConfig>(() => {
  if (!props.toggleable) return chartConfig.value
  const out: ChartConfig = {}
  for (const key of shownKeys.value) {
    if (chartConfig.value[key]) out[key] = chartConfig.value[key]
  }
  return out
})

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

const currencyFormatter = computed(
  () =>
    new Intl.NumberFormat(undefined, {
      style: 'currency',
      currency: props.currency,
      currencyDisplay: 'narrowSymbol',
      maximumFractionDigits: 2,
    }),
)
const formatPrice = (tick: number | Date) => currencyFormatter.value.format(Number(tick))

// Rich tooltip rendered from the shadcn primitive (built once during setup).
const tooltipTemplate = computed(() =>
  componentToString(visibleConfig.value, ChartTooltipContent, {
    labelFormatter: formatDate,
    indicator: 'line',
  }),
)
</script>

<template>
  <div>
    <ChartContainer :config="visibleConfig" class="aspect-auto h-64 w-full" :cursor="true">
      <VisXYContainer :data="points" :margin="{ left: 8, right: 8 }">
        <VisLine v-if="shownUsd" :x="x" :y="usdY" color="var(--chart-1)" :line-width="2" />
        <VisLine v-if="shownFoil" :x="x" :y="foilY" color="var(--chart-2)" :line-width="2" />
        <VisScatter
          v-if="showUsdDot && shownUsd"
          :x="x"
          :y="usdY"
          color="var(--chart-1)"
          :size="36"
        />
        <VisScatter
          v-if="showFoilDot && shownFoil"
          :x="x"
          :y="foilY"
          color="var(--chart-2)"
          :size="36"
        />
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
    <ChartSeriesLegend v-if="showLegend" :items="legendItems" @toggle="toggleSeries" />
  </div>
</template>
