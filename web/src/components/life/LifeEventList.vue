<script setup lang="ts">
import { computed } from 'vue'
import { Undo2 } from '@lucide/vue'
import type { LifeEvent, LifeSeat } from '@/lib/api'
import { describeChange, elapsedLabel, seatColor } from '@/lib/lifeSeries'

// The gain/loss ledger: every recorded change, newest first, each one undoable.
//
// Newest first because during a game the question is always "what just happened" — and because
// the mis-tap you want to undo is almost always the last one. Any row can still be undone, not
// only the newest: the server re-folds the seat's chain, so removing a change from three turns
// ago leaves every later total right.
const props = defineProps<{
  events: LifeEvent[]
  seats: LifeSeat[]
  startedAt: string
  /** False on a finished game — its history is what the deck records were computed from. */
  undoable: boolean
  busy?: boolean
}>()

const emit = defineEmits<{ undo: [eventId: number] }>()

const seatById = computed(() => new Map(props.seats.map((seat) => [seat.id, seat])))
const origin = computed(() => {
  const parsed = Date.parse(props.startedAt)
  return Number.isNaN(parsed) ? 0 : parsed
})

interface Row {
  event: LifeEvent
  name: string
  position: number
  at: string
  /** What the row reads as, counter and damage source included. */
  change: string
  /** The running total to show — only a `life` row's `life_after` is a life total. */
  total: number | null
}

const rows = computed<Row[]>(() =>
  props.events
    .map((event) => {
      const seat = seatById.value.get(event.player_id)
      const at = Date.parse(event.created_at)
      return {
        event,
        // A change whose seat has since been removed still belongs in the log — name it
        // honestly rather than dropping the row.
        name: seat?.name ?? 'Removed player',
        position: seat?.position ?? 0,
        at: elapsedLabel(Math.max(0, (Number.isNaN(at) ? origin.value : at) - origin.value)),
        change: describeChange(
          event,
          event.source_player_id === null
            ? undefined
            : seatById.value.get(event.source_player_id)?.name,
        ),
        // Since #595 `life_after` holds whichever counter the row names, so printing it in the
        // life column would report a 7-point commander hit as a life total of 7. The phrase
        // already carries the counter's own number.
        total: event.counter === 'life' ? event.life_after : null,
      }
    })
    .reverse(),
)
</script>

<template>
  <div>
    <p v-if="!rows.length" class="text-muted-foreground py-6 text-center text-sm">
      No life changes yet. Tap a player's tile to start the history.
    </p>
    <ul v-else class="divide-y">
      <li v-for="row in rows" :key="row.event.id" class="flex items-center gap-3 py-2">
        <span
          class="size-2 shrink-0 rounded-full"
          :style="{ backgroundColor: seatColor(row.position) }"
          aria-hidden="true"
        />
        <span class="min-w-0 flex-1 truncate text-sm">
          <span class="font-medium">{{ row.name }}</span>
          <!-- `&nbsp;`, not a plain space: the compiler drops a whitespace-only text node at the
               start of an element, which ran the name straight into the change ("Player 4lost 3").
               A margin would have fixed the look and left the row reading (and copying) as one
               word, so the gap belongs in the text. -->
          <span class="text-muted-foreground">&nbsp;{{ row.change }}</span>
        </span>
        <span class="text-muted-foreground w-12 shrink-0 text-right text-sm tabular-nums">
          {{ row.total ?? '' }}
        </span>
        <span class="text-muted-foreground w-12 shrink-0 text-right text-xs tabular-nums">
          {{ row.at }}
        </span>
        <button
          v-if="undoable"
          type="button"
          class="text-muted-foreground hover:text-foreground hover:bg-accent grid size-7 shrink-0 place-items-center rounded-md disabled:opacity-50"
          :disabled="busy"
          :aria-label="`Undo: ${row.name} ${row.change}`"
          @click="emit('undo', row.event.id)"
        >
          <Undo2 class="size-4" aria-hidden="true" />
        </button>
      </li>
    </ul>
  </div>
</template>
