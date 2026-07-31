<script setup lang="ts">
import { computed } from 'vue'
import { CurveType } from '@unovis/ts'
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
import {
  clockLabel,
  lifeExtent,
  seatColor,
  stepLabel,
  type LifeLine,
  type LifeStep,
} from '@/lib/lifeSeries'

// The unovis-backed body of the life history chart, split out of LifeHistoryChart so unovis stays
// off the life route's critical chunk (the mat has to be tappable long before anyone scrolls down
// to the history). The wrapper owns the fold into steps and the nothing-to-draw branch; this plots
// what it's handed and owns the per-seat show/hide state behind the legend.
//
// Built on the same `ui/chart` primitives as the collection value chart, so the interaction is the
// same one: a crosshair that snaps to the nearest change, a tooltip naming every seat's total
// there, and a clickable legend that switches a line off. What's different is the x axis — it
// counts *changes*, evenly (see `lifeSteps`), so a break in the game isn't a gap in the chart.
const props = defineProps<{ lines: LifeLine[]; steps: LifeStep[] }>()

/**
 * Series key for a seat. Seat ids are stable for the life of a game, so a toggled-off line stays
 * off across a refetch, and the tooltip config can't drift onto another player.
 */
const seriesKey = (playerId: number) => `seat-${playerId}`

// `lifeSteps` gives a seat a column only if its line had a starting point; a line without one is
// in no row, so it gets no series either rather than a legend entry for nothing.
const drawn = computed(() => props.lines.filter((line) => line.points.length > 0))

// One flat row per step: the step index on x plus one field per seat, keyed by its series key.
// That's the shape ChartTooltipContent reads — it lists whichever fields the config names and
// ignores the rest, so `step` rides along without showing up as a tooltip row.
type Row = Record<string, number>
const rows = computed<Row[]>(() =>
  props.steps.map((step) => {
    const row: Row = { step: step.step }
    for (const line of drawn.value) {
      const life = step.lives[line.playerId]
      if (life !== undefined) row[seriesKey(line.playerId)] = life
    }
    return row
  }),
)

// --- Per-line visibility -------------------------------------------------------------------
// Same engine as the price/value chart's legend (which line stays drawn, the keep-one-line rule).
const dataKeys = computed(() => drawn.value.map((line) => seriesKey(line.playerId)))
const { shownKeys, canToggle, isShown, toggle } = useSeriesToggle(dataKeys)
const shownLines = computed(() => drawn.value.filter((line) => isShown(seriesKey(line.playerId))))

const chartConfig = computed<ChartConfig>(() => {
  const config: ChartConfig = {}
  for (const line of drawn.value) {
    config[seriesKey(line.playerId)] = { label: line.name, color: seatColor(line.position) }
  }
  return config
})

// The tooltip mirrors the drawn lines: a hidden seat drops its row rather than showing a total
// for a line that isn't there.
const visibleConfig = computed<ChartConfig>(() => {
  const out: ChartConfig = {}
  for (const key of shownKeys.value) {
    if (chartConfig.value[key]) out[key] = chartConfig.value[key]
  }
  return out
})

const legendItems = computed(() =>
  drawn.value.map((line) => ({
    key: seriesKey(line.playerId),
    label: line.name,
    color: seatColor(line.position),
    visible: isShown(seriesKey(line.playerId)),
  })),
)

// One accessor per drawn seat, rebuilt only when the shown set changes — a fresh closure every
// render would make unovis redraw every line on every tick.
const series = computed(() =>
  shownLines.value.map((line) => {
    const key = seriesKey(line.playerId)
    return {
      key,
      color: seatColor(line.position),
      y: (d: Row) => d[key] ?? null,
    }
  }),
)

const x = (d: Row) => d.step ?? 0

// The crosshair is given its accessors explicitly rather than inheriting them from the plotted
// components: the fallback flattens every component's `y` in registration order, and the circle
// colours below are indexed by that order — reading it off `series` is the only way the dot on a
// line is guaranteed to be that line's colour.
const crosshairY = computed(() => series.value.map((entry) => entry.y))
const crosshairColors = computed(() => series.value.map((entry) => entry.color))

// The y range follows what's *drawn*, so hiding the player who went to zero reclaims the space
// their line was using. `lifeExtent` gives a game where nothing has moved yet some height rather
// than drawing the line on the axis.
const extent = computed(() => lifeExtent(shownLines.value))
const yDomain = computed<[number, number]>(() => [extent.value.min, extent.value.max])

// Life is a whole number, and the domain is pinned to the exact extent, so d3's own tick choice
// would hand back halves on a narrow game (39, 39.5, 40) that round to a repeated label. Pick
// integers across the range instead — at most five, deduped, so a 3-life span shows 3 ticks.
const AXIS_TICKS = 5
function evenTicks(from: number, to: number, limit: number): number[] {
  const span = to - from
  if (span <= 0) return [from]
  const count = Math.min(limit, span + 1)
  return [
    ...new Set(
      Array.from({ length: count }, (_, i) => Math.round(from + (i * span) / (count - 1))),
    ),
  ]
}
const lifeTickValues = computed(() => evenTicks(extent.value.min, extent.value.max, AXIS_TICKS))
const formatLife = (tick: number | Date) => String(Math.round(Number(tick)))

// x ticks are step *indices*, so they have to be pinned to real integer steps — a tick at 2.5
// names no change. Up to five, spread evenly, each labelled with the clock at that change: the
// axis is even in changes and still says when they happened.
const tickValues = computed(() => evenTicks(0, props.steps.length - 1, AXIS_TICKS))
const formatStepTick = computed(() => {
  const steps = props.steps
  return (tick: number | Date) => {
    const step = steps[typeof tick === 'number' ? Math.round(tick) : Number(tick)]
    return step ? clockLabel(step.at) : ''
  }
})

// A game whose whole history is one column has no line to stroke — draw the dots so it isn't a
// blank frame. (The wrapper only mounts this with two or more steps; this keeps the component
// honest on its own terms.)
const showDots = computed(() => rows.value.length === 1)

// Rich tooltip over the shared primitive. The crosshair hands the template the *snapped* datum's
// x — an exact step index — so the heading can name the change and its clock.
const tooltipTemplate = computed(() => {
  const steps = props.steps
  return componentToString(visibleConfig.value, ChartTooltipContent, {
    labelFormatter: (tick: number | Date) => {
      const step = steps[typeof tick === 'number' ? Math.round(tick) : Number(tick)]
      return step ? stepLabel(step) : ''
    },
    indicator: 'line',
  })
})

/** A plain-text read of the chart for anyone who can't see it: where each seat ended up. */
const summary = computed(() =>
  drawn.value
    .map((line) => `${line.name}: ${line.points[line.points.length - 1]?.life ?? 0}`)
    .join(', '),
)
</script>

<template>
  <div>
    <ChartContainer :config="visibleConfig" class="aspect-auto h-48 w-full" :cursor="true">
      <!-- The y domain is the exact life extent, so the top line would be drawn on the frame's
           edge; the margin is what keeps its stroke inside the plot. -->
      <VisXYContainer :data="rows" :margin="{ top: 6, left: 8, right: 8 }" :y-domain="yDomain">
        <!-- A step, not a slope: life doesn't drift between changes, it jumps at one. -->
        <VisLine
          v-for="entry in series"
          :key="entry.key"
          :x="x"
          :y="entry.y"
          :color="entry.color"
          :curve-type="CurveType.StepAfter"
          :line-width="2"
        />
        <template v-if="showDots">
          <VisScatter
            v-for="entry in series"
            :key="`dot-${entry.key}`"
            :x="x"
            :y="entry.y"
            :color="entry.color"
            :size="36"
          />
        </template>
        <VisAxis
          type="x"
          :x="x"
          :tick-values="tickValues"
          :tick-format="formatStepTick"
          :grid-line="false"
          :tick-line="false"
          :domain-line="false"
        />
        <VisAxis
          type="y"
          :tick-values="lifeTickValues"
          :tick-format="formatLife"
          :grid-line="true"
          :tick-line="false"
          :domain-line="false"
        />
        <!-- `hideWhenFarFromPointer` defaults on, at 100px: the crosshair (and its tooltip)
             blanks out whenever the snapped column is further than that from the pointer. On a
             dense price series that never fires, but a column here is one *change*, so a short
             game spaces them hundreds of pixels apart and the middle of every interval would
             answer a hover with nothing. Off — the pointer is always nearest to *some* change,
             and leaving the plot still hides it (the range checks are separate). -->
        <ChartCrosshair
          :x="x"
          :y="crosshairY"
          :color="crosshairColors"
          :hide-when-far-from-pointer="false"
          :template="tooltipTemplate"
        />
        <ChartTooltip />
      </VisXYContainer>
    </ChartContainer>
    <p class="sr-only">Final totals — {{ summary }}.</p>
    <!-- Names carry the identity; the swatch only ties a name to its line. With two or more seats
         each name is also the switch for that line. A lone seat has nothing to switch between, so
         it keeps the plain caption rather than a button that would refuse its own click. -->
    <ChartSeriesLegend v-if="canToggle" :items="legendItems" @toggle="toggle" />
    <p v-else class="mt-3 flex flex-wrap justify-center gap-x-4 gap-y-1">
      <span
        v-for="line in drawn"
        :key="line.playerId"
        class="text-muted-foreground flex items-center gap-1.5 text-xs"
      >
        <span
          class="size-2 rounded-full"
          :style="{ backgroundColor: seatColor(line.position) }"
          aria-hidden="true"
        />
        {{ line.name }}
      </span>
    </p>
  </div>
</template>
