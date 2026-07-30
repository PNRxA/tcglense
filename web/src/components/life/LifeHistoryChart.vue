<script setup lang="ts">
import { computed } from 'vue'
import { lifeDuration, lifeExtent, seatColor, type LifeLine } from '@/lib/lifeSeries'

// Every seat's life over the whole game, as one step chart.
//
// Hand-drawn SVG rather than the shared unovis `PriceChart`: that component is built around a
// price series with set-release markers and a foil/regular toggle, and reusing it would mean
// bending it into a shape it isn't (n series, a game-relative x axis, integer life on y). A
// step polyline per seat with a shared axis is ~60 lines and stays exactly what it is.
//
// **Time-spaced here** (unlike the per-tile sparkline, which is index-spaced): at full width the
// gaps between changes are the interesting part — you can see the turn where three players
// traded blows and the ten quiet minutes after it.
const props = defineProps<{ lines: LifeLine[] }>()

const WIDTH = 600
const HEIGHT = 200
const PAD = { top: 8, right: 8, bottom: 18, left: 30 }

const extent = computed(() => lifeExtent(props.lines))
const duration = computed(() => lifeDuration(props.lines))

const plot = computed(() => ({
  width: WIDTH - PAD.left - PAD.right,
  height: HEIGHT - PAD.top - PAD.bottom,
}))

function xAt(at: number): number {
  // A game with one instant of history still draws: everything sits at the left edge.
  const span = duration.value || 1
  return PAD.left + (at / span) * plot.value.width
}

function yAt(life: number): number {
  const { min, max } = extent.value
  const span = max - min || 1
  return PAD.top + (1 - (life - min) / span) * plot.value.height
}

const series = computed(() =>
  props.lines
    .filter((line) => line.points.length > 0)
    .map((line) => {
      const points = line.points
      let d = `M ${xAt(points[0]?.at ?? 0)} ${yAt(points[0]?.life ?? 0)}`
      points.forEach((point, index) => {
        if (index === 0) return
        d += ` H ${xAt(point.at)} V ${yAt(point.life)}`
      })
      // Extend the last total to the right edge, so the line reads as "still there" rather
      // than stopping mid-chart at the last change.
      d += ` H ${xAt(duration.value)}`
      return { line, d, color: seatColor(line.position) }
    }),
)

/** Three y gridlines: the two bounds and the middle, enough to read a total off. */
const ticks = computed(() => {
  const { min, max } = extent.value
  const mid = Math.round((min + max) / 2)
  return [...new Set([max, mid, min])].map((life) => ({ life, y: yAt(life) }))
})
</script>

<template>
  <figure class="m-0">
    <svg
      :viewBox="`0 0 ${WIDTH} ${HEIGHT}`"
      class="h-48 w-full"
      role="img"
      :aria-label="`Life totals over time for ${lines.map((l) => l.name).join(', ')}`"
    >
      <g class="text-muted-foreground">
        <template v-for="tick in ticks" :key="tick.life">
          <line
            :x1="PAD.left"
            :x2="WIDTH - PAD.right"
            :y1="tick.y"
            :y2="tick.y"
            stroke="currentColor"
            stroke-width="1"
            opacity="0.15"
          />
          <text
            :x="PAD.left - 6"
            :y="tick.y + 3"
            text-anchor="end"
            fill="currentColor"
            class="text-[9px] tabular-nums"
            opacity="0.7"
          >
            {{ tick.life }}
          </text>
        </template>
      </g>
      <path
        v-for="entry in series"
        :key="entry.line.playerId"
        :d="entry.d"
        :stroke="entry.color"
        stroke-width="2"
        stroke-linejoin="round"
        fill="none"
        vector-effect="non-scaling-stroke"
      />
    </svg>
    <!-- Names carry the identity; the swatch only ties a name to its line. -->
    <figcaption class="mt-2 flex flex-wrap gap-x-4 gap-y-1">
      <span
        v-for="line in lines"
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
    </figcaption>
  </figure>
</template>
