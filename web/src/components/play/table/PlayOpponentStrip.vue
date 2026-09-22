<script setup lang="ts">
import { computed } from 'vue'
import { EyeOff, Hand, Layers, WifiOff } from '@lucide/vue'
import { Sheet, SheetContent, SheetTitle } from '@/components/ui/sheet'
import PlayCard from '@/components/play/table/PlayCard.vue'
import PlayOpponentBoard from '@/components/play/table/PlayOpponentBoard.vue'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { seatColor } from '@/lib/playTable'
import type { PlayCardView, PlaySeatSnapshot } from '@/lib/api/play'

// The other players, on a phone.
//
// A full opponent board is ~200px tall; three of them is the whole screen. But you do not read
// an opponent's board continuously — you read their **life**, and you look at their board when
// something happens on it. So the phone gets a chip per seat: a commander thumb, the name, the
// life, and the counts that decide whether you can act (hand, library, and how many face-down
// permanents they have, because "two morphs" is a threat and a `def: null` card is exactly how
// the server tells us one exists without telling us what it is).
//
// Tapping a chip opens that opponent's real board in a sheet — the same `PlayOpponentBoard` the
// desktop shows, so their battlefield, graveyard and exile all work from there. The strip
// itself never grows past one chip's height; it scrolls sideways.
const table = usePlayTableContext()
const store = table.store

function commander(seat: PlaySeatSnapshot): PlayCardView | null {
  return (store.cardsIn(seat.id, 'command') as PlayCardView[])[0] ?? null
}

/** How many of their permanents this viewer is allowed to know exist but not identify. */
function faceDownCount(seat: PlaySeatSnapshot): number {
  return (store.cardsIn(seat.id, 'battlefield') as PlayCardView[]).filter(
    (card) => card.def === null || card.face_down,
  ).length
}

const openSeat = computed(() =>
  table.openOpponent.value === null ? null : (store.seatById(table.openOpponent.value) ?? null),
)
</script>

<template>
  <div
    v-if="store.opponents.length > 0"
    class="flex shrink-0 gap-1.5 overflow-x-auto"
    aria-label="Opponents"
  >
    <button
      v-for="seat in store.opponents"
      :key="seat.id"
      type="button"
      class="bg-card hover:border-ring/60 flex w-40 shrink-0 items-center gap-1.5 rounded-lg border p-1 text-left transition-colors"
      :class="[
        store.turn?.active_seat === seat.id ? 'border-primary/60' : '',
        seat.out ? 'opacity-60' : '',
      ]"
      :aria-label="`${seat.name}, ${seat.life} life — open their board`"
      @click="table.openOpponent.value = seat.id"
    >
      <span class="w-7 shrink-0">
        <PlayCard
          v-if="commander(seat)"
          :card="commander(seat)!"
          :game="store.game"
          size="small"
          :interactive="false"
        />
        <span
          v-else
          class="bg-muted/40 block aspect-[61/85] w-full rounded-sm border border-dashed"
          aria-hidden="true"
        />
      </span>
      <span class="min-w-0 flex-1">
        <span class="flex items-center gap-1">
          <span
            class="size-1.5 shrink-0 rounded-full"
            :style="{ backgroundColor: seatColor(seat.seat_index) }"
            aria-hidden="true"
          />
          <span class="min-w-0 flex-1 truncate text-[0.7rem] leading-tight">{{ seat.name }}</span>
          <WifiOff
            v-if="!seat.connected"
            class="text-muted-foreground size-3 shrink-0"
            aria-hidden="true"
          />
        </span>
        <span class="flex items-baseline gap-1">
          <span
            class="text-base leading-tight font-semibold tabular-nums"
            :class="seat.life <= 0 ? 'text-destructive' : seat.life <= 5 ? 'text-warning' : ''"
            >{{ seat.life }}</span
          >
          <span
            class="text-muted-foreground flex items-center gap-0.5 truncate text-[0.6rem] tabular-nums"
          >
            <Hand class="size-2.5" aria-hidden="true" />{{ seat.hand_count }}
            <Layers class="ml-0.5 size-2.5" aria-hidden="true" />{{ seat.library_count }}
          </span>
        </span>
      </span>
      <span
        v-if="faceDownCount(seat) > 0"
        class="text-muted-foreground bg-muted flex shrink-0 items-center gap-0.5 rounded px-1 text-[0.6rem] tabular-nums"
        :aria-label="`${faceDownCount(seat)} face down`"
      >
        <EyeOff class="size-3" aria-hidden="true" />{{ faceDownCount(seat) }}
      </span>
    </button>
  </div>

  <!-- The real board, when you actually want to look at it. -->
  <Sheet
    :open="openSeat !== null"
    @update:open="(value: boolean) => !value && (table.openOpponent.value = null)"
  >
    <SheetContent side="bottom" class="max-h-[85dvh] overflow-y-auto p-3">
      <SheetTitle class="text-sm">{{ openSeat?.name ?? 'Opponent' }}</SheetTitle>
      <PlayOpponentBoard v-if="openSeat" :seat="openSeat" full class="mt-2" />
    </SheetContent>
  </Sheet>
</template>
