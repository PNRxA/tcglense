<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { VisAxis, VisLine, VisPlotline, VisScatter, VisXYContainer } from '@unovis/vue'
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
import { useChartSetMarkers, type PlotGeometry } from '@/composables/useChartSetMarkers'
import { setIconUrl, type CardSet, type PriceRange } from '@/lib/api'
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
  /** The game's full set list, for the set-release markers. Empty (the default) draws no
   * markers, so a chart without a game stays exactly as before. */
  sets?: CardSet[]
  /** Game id for the marker logos' icon-proxy URLs; markers only draw when it's set. */
  game?: string
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

// --- Set-release markers (opt-in via `game` + `sets`) -------------------------------------
// Vertical flags where a notable set dropped, so a jump or dip on the line reads against the
// release that may have driven it. The plotted x-window is the extent of the series' days; we
// pin the x-domain to it so the marker overlay's pixel math matches unovis' scale exactly, and
// read the rendered plot geometry from `onRenderComplete` to place each set's logo on its line.
// The selection + layout live in the useChartSetMarkers composable (pure, unit-tested); this
// wires the reactive inputs and holds the render geometry.
const xExtent = computed<[number, number]>(() => {
  const xs = points.value.map((p) => p.date)
  return xs.length ? [Math.min(...xs), Math.max(...xs)] : [0, 0]
})
const xMin = computed(() => xExtent.value[0])
const xMax = computed(() => xExtent.value[1])
// Markers need a real game, some sets, and a non-degenerate window to map across.
const markersEnabled = computed(
  () => !!props.game && (props.sets?.length ?? 0) > 0 && xMax.value > xMin.value,
)

const setsRef = computed<CardSet[]>(() => props.sets ?? [])
const geometry = ref<PlotGeometry | null>(null)
const { positioned: setMarkers } = useChartSetMarkers(setsRef, xMin, xMax, geometry)

// unovis reports the computed margins (axis + label width) and the plot area size after each
// render/resize; combined with the pinned domain that's everything the overlay needs to sit a
// logo exactly on its plotline. `bleed` is unovis' internal overflow padding, folded into the
// left offset so the origin matches the drawn scale.
// unovis' margin/bleed spacing shape (avoids a fragile deep type import).
type Spacing = { left?: number; right?: number; top?: number; bottom?: number }
function onRenderComplete(
  _svg: SVGSVGElement,
  margin: Spacing,
  bleed: Spacing,
  _containerWidth: number,
  _containerHeight: number,
  componentWidth: number,
) {
  if (!markersEnabled.value) {
    if (geometry.value) geometry.value = null
    return
  }
  const next: PlotGeometry = {
    marginLeft: (margin.left ?? 0) + (bleed.left ?? 0),
    plotWidth: componentWidth,
    xMin: xMin.value,
    xMax: xMax.value,
  }
  // Drawing the plotlines makes unovis re-render, which fires this again with identical
  // geometry — bail on an unchanged value so we don't churn reactively in a loop.
  const prev = geometry.value
  if (
    prev &&
    prev.marginLeft === next.marginLeft &&
    prev.plotWidth === next.plotWidth &&
    prev.xMin === next.xMin &&
    prev.xMax === next.xMax
  ) {
    return
  }
  geometry.value = next
}

// Set icons ship as monochrome silhouettes (`dark:invert` flips them for the dark theme,
// matching SetTile); a logo that fails to load falls back to the plain plotline cap.
const failedIcons = ref<Set<string>>(new Set())
function onIconError(code: string) {
  failedIcons.value = new Set(failedIcons.value).add(code)
}

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
    <!-- Relative wrapper so the set-logo overlay can be absolutely positioned over the plot. -->
    <div class="relative">
      <ChartContainer :config="visibleConfig" class="aspect-auto h-64 w-full" :cursor="true">
        <VisXYContainer
          :data="points"
          :margin="{ left: 8, right: 8 }"
          :x-domain="markersEnabled ? [xMin, xMax] : undefined"
          :on-render-complete="onRenderComplete"
        >
          <!-- Set-release rules, drawn first so they sit beneath the price lines: a subtle
               dashed vertical at each notable release in the window (logos are the overlay
               below, positioned on these same x values). -->
          <VisPlotline
            v-for="m in setMarkers"
            :key="m.code"
            axis="x"
            :value="m.x"
            color="color-mix(in oklab, var(--muted-foreground) 55%, transparent)"
            :line-width="1.5"
            line-style="dash"
          />
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

      <!-- Set markers: each release's logo above its uppercased set code, positioned on the
           plotline from the rendered plot geometry, linking to that set's page. The layer
           ignores pointer events so it never eats a crosshair hover; each chip re-enables them
           for its own click + hover label. The silhouette is decorative (meaning carried in the
           link's label + the code); a logo that fails to load leaves the code (still a link)
           over its dashed plotline. -->
      <div
        v-if="markersEnabled"
        class="pointer-events-none absolute inset-0 overflow-hidden"
        data-testid="set-markers"
      >
        <template v-for="m in setMarkers" :key="m.code">
          <RouterLink
            v-if="m.showIcon && game && !failedIcons.has(m.code)"
            :to="`/cards/${game}/sets/${m.code}`"
            class="group focus-visible:ring-ring/50 pointer-events-auto absolute top-1 flex -translate-x-1/2 flex-col items-center gap-0.5 rounded focus-visible:ring-2 focus-visible:outline-none"
            :style="{ left: `${m.left}px` }"
            :title="`${m.name} · released ${formatDate(m.x)}`"
            :aria-label="`${m.name} (${m.code}), released ${formatDate(m.x)} — view set`"
          >
            <img
              :src="setIconUrl(game, m.code)"
              alt=""
              class="size-4 object-contain opacity-70 transition-opacity group-hover:opacity-100 dark:invert"
              loading="lazy"
              @error="onIconError(m.code)"
            />
            <span
              class="text-muted-foreground group-hover:text-foreground text-[9px] leading-none font-semibold tracking-wide whitespace-nowrap uppercase transition-colors"
            >
              {{ m.code }}
            </span>
          </RouterLink>
        </template>
      </div>
    </div>
    <ChartSeriesLegend v-if="showLegend" :items="legendItems" @toggle="toggleSeries" />
  </div>
</template>
