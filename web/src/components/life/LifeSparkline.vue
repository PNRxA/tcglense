<script setup lang="ts">
import { computed } from 'vue'
import { lifeExtent, seatColor, type LifeLine } from '@/lib/lifeSeries'

// One seat's life over the game, as a step line inside its own tile.
//
// Hand-drawn SVG rather than the shared chart primitive on purpose: this renders once per seat
// on a surface that must stay responsive under rapid taps, it has no axes, legend, tooltip or
// interaction, and it has to survive being rotated with its tile. A chart library here would be
// a lazily-loaded bundle to draw a polyline.
//
// A step (not a slope) is the honest shape: life doesn't drift between changes, it jumps at one.
const props = defineProps<{ line: LifeLine | undefined; position: number }>()

const WIDTH = 100
const HEIGHT = 24

/** The step path, in a 100x24 viewBox stretched to the tile via a non-uniform preserveAspectRatio. */
const path = computed(() => {
  const points = props.line?.points ?? []
  // One point is a game where nothing has happened yet: draw the flat line rather than nothing,
  // so the tile doesn't look broken before the first tap.
  if (points.length === 0) return ''
  const { min, max } = lifeExtent([{ ...(props.line as LifeLine), points }])
  const span = max - min || 1
  const y = (life: number) => HEIGHT - ((life - min) / span) * HEIGHT
  // Index-spaced, not time-spaced: on a tile this small, time spacing collapses a flurry of
  // changes into one pixel and leaves the rest of the line empty.
  const x = (index: number) => (points.length === 1 ? WIDTH : (index / (points.length - 1)) * WIDTH)
  let d = `M ${x(0)} ${y(points[0]?.life ?? 0)}`
  points.forEach((point, index) => {
    if (index === 0) return
    // Horizontal to the change, then vertical through it — the step.
    d += ` H ${x(index)} V ${y(point.life)}`
  })
  if (points.length === 1) d += ` H ${WIDTH}`
  return d
})

const stroke = computed(() => seatColor(props.position))
</script>

<template>
  <svg
    v-if="path"
    :viewBox="`0 0 ${WIDTH} ${HEIGHT}`"
    preserveAspectRatio="none"
    class="h-full w-full"
    aria-hidden="true"
    focusable="false"
  >
    <path
      :d="path"
      :stroke="stroke"
      stroke-width="1.5"
      fill="none"
      vector-effect="non-scaling-stroke"
    />
  </svg>
</template>
