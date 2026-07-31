<script setup lang="ts">
import LifeEventList from '@/components/life/LifeEventList.vue'
import LifeHistoryChart from '@/components/life/LifeHistoryChart.vue'
import type { LifeEvent, LifeSeat } from '@/lib/api'
import type { LifeLine } from '@/lib/lifeSeries'

// The game's history in full: the shape of it (chart) above the detail of it (ledger).
//
// Presentational only — the chart and the list are each their own component, and this is the
// frame that pairs them, so the mat page and the finished scoreboard show the same thing without
// either re-assembling it.
defineProps<{
  lines: LifeLine[]
  events: LifeEvent[]
  seats: LifeSeat[]
  startedAt: string
  undoable: boolean
  busy?: boolean
}>()

const emit = defineEmits<{ undo: [eventId: number] }>()
</script>

<template>
  <section class="space-y-4">
    <div v-if="events.length" class="bg-card rounded-xl border p-4">
      <h3 class="mb-3 text-sm font-medium">Life over the game</h3>
      <LifeHistoryChart :lines="lines" />
    </div>
    <div class="bg-card rounded-xl border px-4 py-2">
      <h3 class="py-2 text-sm font-medium">History</h3>
      <LifeEventList
        :events="events"
        :seats="seats"
        :started-at="startedAt"
        :undoable="undoable"
        :busy="busy"
        @undo="(id) => emit('undo', id)"
      />
    </div>
  </section>
</template>
