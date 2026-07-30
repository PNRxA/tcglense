<script setup lang="ts">
import LifeSeatTile from '@/components/life/LifeSeatTile.vue'
import { seatCellStyle } from '@/lib/lifeLayout'
import type { LifeSeatView } from '@/composables/useLifeSession'

// The table: one CSS grid, one cell per seat, laid out by `lib/lifeLayout`'s pure placement
// maths (which is where the per-player-count grid rules and rotations are tested).
//
// Each cell is its own **size container** so the tile inside can be sized from the cell's
// height as well as its width — that's what lets a quarter-turned tile fill a cell it is
// rotated inside of, instead of being laid out at the cell's width and then turned.
//
// The tile is positioned **absolutely and centred by `translate`**, not by `place-items-center`:
// a quarter-turned tile's *layout* box is taller than its cell (it's the pre-rotation box), and
// Chromium refuses to centre an overflowing grid item — it pins it to the start edge, which put
// every side seat a half-cell out of place. Centring it ourselves is exact, and because the
// independent `translate` and `rotate` properties are applied in that order by spec, the tile
// ends up rotated about the cell's own centre.
defineProps<{
  seats: LifeSeatView[]
  grid: { gridTemplateColumns: string; gridTemplateRows: string }
  editable: boolean
  winnerId: number | null
  gameSlug: string
}>()

const emit = defineEmits<{
  bump: [playerId: number, delta: number]
  settings: [playerId: number]
}>()
</script>

<template>
  <div class="grid h-full w-full gap-2" :style="grid">
    <div
      v-for="view in seats"
      :key="view.seat.id"
      class="relative min-h-0 min-w-0 overflow-hidden"
      :style="{ ...seatCellStyle(view.placement), containerType: 'size' }"
    >
      <LifeSeatTile
        :view="view"
        :editable="editable"
        :winner="winnerId === view.seat.id"
        :game-slug="gameSlug"
        @bump="(delta) => emit('bump', view.seat.id, delta)"
        @settings="emit('settings', view.seat.id)"
      />
    </div>
  </div>
</template>
