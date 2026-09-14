<script setup lang="ts">
import { computed } from 'vue'
import { Crown, WifiOff } from '@lucide/vue'
import PlayBattlefield from '@/components/play/table/PlayBattlefield.vue'
import PlayCard from '@/components/play/table/PlayCard.vue'
import PlayCardMenu from '@/components/play/table/PlayCardMenu.vue'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { seatColor } from '@/lib/playTable'
import type { PlayCardView, PlaySeatSnapshot, PlayZone } from '@/lib/api/play'

// One opponent, as seen from across the table.
//
// What is shown is decided by the server, not here: their hand and library arrive as **counts**
// (`hand_count` / `library_count`, with no ids at all), their face-down permanents arrive with
// `def: null`. So this board cannot leak what it doesn't have — the privacy rule lives in
// `view.rs`, and the UI simply renders whatever it was given.
//
// Their battlefield is the real thing at a third of the size: same layout maths, same cards,
// same tapped rotation, hover preview intact. Read-only for gestures, but cards keep their
// context menu, because the one legal reach across the table — taking control of a permanent —
// belongs on the card it is about.
const props = withDefaults(
  defineProps<{
    seat: PlaySeatSnapshot
    /** Fill the container instead of taking a fixed slot in the desktop strip (the phone sheet). */
    full?: boolean
  }>(),
  { full: false },
)

const table = usePlayTableContext()

const commanders = computed<PlayCardView[]>(
  () => table.store.cardsIn(props.seat.id, 'command') as PlayCardView[],
)
const damageTaken = computed(() =>
  Object.entries(props.seat.commander_damage)
    .map(([from, value]) => ({ from: Number(from), value }))
    .filter((row) => row.value > 0),
)
const poison = computed(() => props.seat.counters.poison ?? 0)
const isActive = computed(() => table.store.turn?.active_seat === props.seat.id)

function openZone(zone: PlayZone) {
  table.openViewer({ kind: 'zone', seatId: props.seat.id, zone })
}

function sourceName(seatId: number): string {
  return table.store.seatById(seatId)?.name ?? 'a player who left'
}
</script>

<template>
  <article
    class="bg-muted/30 flex flex-col gap-1.5 rounded-lg border p-2"
    :class="[
      full ? 'w-full' : 'w-[22rem] shrink-0 sm:w-[26rem]',
      isActive ? 'border-primary/60' : '',
      seat.out ? 'opacity-60' : '',
    ]"
    :aria-label="`${seat.name}, ${seat.life} life`"
  >
    <div class="flex items-center gap-1.5">
      <span
        class="size-2 shrink-0 rounded-full"
        :style="{ backgroundColor: seatColor(seat.seat_index) }"
        aria-hidden="true"
      />
      <p class="min-w-0 flex-1 truncate text-sm font-medium">
        {{ seat.name }}
        <span v-if="seat.deck_name" class="text-muted-foreground text-xs">
          · {{ seat.deck_name }}</span
        >
      </p>
      <WifiOff
        v-if="!seat.connected"
        class="text-muted-foreground size-3.5 shrink-0"
        aria-label="Disconnected"
      />
      <Crown
        v-if="seat.is_host"
        class="text-muted-foreground size-3.5 shrink-0"
        aria-label="Host"
      />
      <span
        class="text-xl leading-none font-semibold tabular-nums"
        :class="seat.life <= 0 ? 'text-destructive' : seat.life <= 5 ? 'text-warning' : ''"
        >{{ seat.life }}</span
      >
    </div>

    <div class="flex flex-wrap items-center gap-1 text-[0.7rem]">
      <span v-if="poison > 0" class="bg-destructive/15 text-destructive rounded px-1.5 py-0.5">
        {{ poison }} poison
      </span>
      <span
        v-for="row in damageTaken"
        :key="row.from"
        class="bg-warning/15 text-warning rounded px-1.5 py-0.5"
      >
        {{ row.value }} from {{ sourceName(row.from) }}
      </span>
      <span class="bg-muted text-muted-foreground rounded px-1.5 py-0.5">
        Hand {{ seat.hand_count }}
      </span>
      <span class="bg-muted text-muted-foreground rounded px-1.5 py-0.5">
        Library {{ seat.library_count }}
      </span>
      <button
        type="button"
        class="bg-muted text-muted-foreground hover:text-foreground rounded px-1.5 py-0.5"
        @click="openZone('graveyard')"
      >
        Graveyard {{ seat.graveyard.length }}
      </button>
      <button
        type="button"
        class="bg-muted text-muted-foreground hover:text-foreground rounded px-1.5 py-0.5"
        @click="openZone('exile')"
      >
        Exile {{ seat.exile.length }}
      </button>
    </div>

    <div class="flex gap-1.5">
      <div v-if="commanders.length > 0" class="flex w-12 shrink-0 flex-col gap-1">
        <PlayCardMenu v-for="card in commanders" :key="card.id" :card="card" zone="command">
          <PlayCard :card="card" :game="table.store.game" size="small" />
        </PlayCardMenu>
      </div>
      <div class="h-32 min-w-0 flex-1 sm:h-40">
        <PlayBattlefield :seat-id="seat.id" :interactive="false" menu card-width="13%" />
      </div>
    </div>
  </article>
</template>
