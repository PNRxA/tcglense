<script setup lang="ts">
import { computed, ref } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { VisAxis, VisLine, VisScatter, VisXYContainer } from '@unovis/vue'
import {
  type ChartConfig,
  ChartContainer,
  ChartCrosshair,
  ChartTooltip,
  ChartTooltipContent,
  componentToString,
} from '@/components/ui/chart'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { type PriceRange } from '@/lib/api'

// The shared price-history chart + range picker, used by both card and sealed-product
// detail pages. It's fed a `fetcher` (which takes the selected range and returns the
// USD series) and a base `queryKey`; the range is appended to that key so a range
// change refetches under a distinct cache entry. The two USD fields (`usd`/`usd_foil`)
// are all it reads, so any series carrying them — a card's `PricePoint` (with unused
// eur/tix) or a product's `ProductPricePoint` — satisfies it.
interface PricePointLike {
  date: string
  usd: string | null
  usd_foil: string | null
}
const props = defineProps<{
  /** Base cache key (without the range); the selected range is appended. */
  queryKey: readonly unknown[]
  /** Fetch the USD price series for the given range. */
  fetcher: (range: PriceRange) => Promise<{ data: PricePointLike[] }>
}>()

// Selectable time window; longer ranges come back downsampled from the API. We
// default to 30 days — a daily (un-downsampled) window — so a young series shows
// every captured day. The 1y+ ranges bucket to weekly/coarser and keep only the
// last day per bucket, which collapses a handful of same-week days into a single
// point (misreads as "no history" on a fresh deployment).
const range = ref<PriceRange>('30d')
const RANGE_OPTIONS: { value: PriceRange; label: string }[] = [
  { value: '7d', label: '7D' },
  { value: '30d', label: '30D' },
  { value: '1y', label: '1Y' },
  { value: '2y', label: '2Y' },
  { value: '3y', label: '3Y' },
  { value: 'all', label: 'All' },
]
function selectRange(value: PriceRange) {
  range.value = value
}

// Public price-history endpoint, so a plain useQuery (no auth wrapper). Refs go
// straight into the queryKey so a card-to-card navigation (or a range change)
// refetches; keepPreviousData holds the current chart on screen while the next
// range loads instead of flashing the loading skeleton.
const query = useQuery({
  queryKey: computed(() => [...props.queryKey, range.value]),
  queryFn: () => props.fetcher(range.value),
  placeholderData: keepPreviousData,
})

// One plotted day. Dates become epoch ms for a continuous x-scale; price strings
// become numbers, with null kept as null so the line *gaps* over missing days
// rather than dropping to zero.
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
  (query.data.value?.data ?? []).map((p) => ({
    date: new Date(p.date).getTime(),
    usd: toNumber(p.usd),
    usdFoil: toNumber(p.usd_foil),
  })),
)

const isEmpty = computed(
  () => !query.isPending.value && !query.isError.value && points.value.length === 0,
)

// No point markers: the crosshair snaps to the nearest datum on hover, so the dots
// are redundant. The one exception is a single-datum series, which has no line
// stroke to draw — without a dot it'd render nothing at all.
const showDots = computed(() => points.value.length === 1)

// Series legend/tooltip metadata. Colours are the theme's chart tokens, which the
// CSS variables resolve differently in light vs dark, so the chart follows the theme.
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
  const fmt = range.value === '7d' || range.value === '30d' ? axisDateShort : axisDateLong
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
  <Card class="mt-6">
    <CardHeader>
      <div class="flex flex-wrap items-center justify-between gap-2">
        <CardTitle class="text-sm font-semibold">Price history</CardTitle>
        <div
          class="bg-muted/50 inline-flex items-center gap-1 rounded-lg p-0.5"
          role="group"
          aria-label="Price history range"
        >
          <Button
            v-for="opt in RANGE_OPTIONS"
            :key="opt.value"
            type="button"
            :variant="range === opt.value ? 'secondary' : 'ghost'"
            size="sm"
            class="h-8 px-2.5 text-xs font-medium"
            :aria-pressed="range === opt.value"
            @click="selectRange(opt.value)"
          >
            {{ opt.label }}
          </Button>
        </div>
      </div>
    </CardHeader>
    <CardContent>
      <div
        v-if="query.isPending.value"
        class="bg-muted/40 h-64 w-full animate-pulse rounded-xl"
        aria-hidden="true"
      />
      <p v-else-if="query.isError.value" class="text-muted-foreground py-12 text-sm">
        Couldn't load price history.
      </p>
      <p v-else-if="isEmpty" class="text-muted-foreground py-12 text-sm">
        No price history for this range.
      </p>

      <ChartContainer v-else :config="chartConfig" class="aspect-auto h-64 w-full" :cursor="true">
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
    </CardContent>
  </Card>
</template>
