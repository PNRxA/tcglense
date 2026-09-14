<script setup lang="ts">
import { computed } from 'vue'
import PlayCard from '@/components/play/table/PlayCard.vue'
import PlayCardMenu from '@/components/play/table/PlayCardMenu.vue'
import PlayZonePile from '@/components/play/table/PlayZonePile.vue'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { cardName } from '@/lib/playTable'
import type { PlayCardView } from '@/lib/api/play'

// My own zones, as a rail down the left of the board.
//
// The library is the only pile with a menu rather than an action, because it is the only one
// with more than one obvious thing to do to it — draw, look, search, shuffle, mulligan — and
// because every one of those is a decision you make deliberately. The other piles open a
// viewer, which is what "look through my graveyard" means.
//
// The command zone is not a pile at all: commanders are few, permanently relevant, and the
// thing you do to them is cast them, so they are shown as cards you can click.
const table = usePlayTableContext()

const seat = computed(() => table.store.mySeat)
const seatId = computed(() => table.store.mySeatId)

function pileTop(zone: 'graveyard' | 'exile'): PlayCardView | null {
  const id = seatId.value
  if (id === null) return null
  return (table.store.cardsIn(id, zone) as PlayCardView[])[0] ?? null
}

const graveyard = computed(() => pileTop('graveyard'))
const exile = computed(() => pileTop('exile'))
const commanders = computed<PlayCardView[]>(() =>
  seatId.value === null ? [] : (table.store.cardsIn(seatId.value, 'command') as PlayCardView[]),
)
const showsCommandZone = computed(
  () => table.store.format === 'commander' || commanders.value.length > 0,
)

function openZone(zone: 'graveyard' | 'exile') {
  if (seatId.value === null) return
  table.openViewer({ kind: 'zone', seatId: seatId.value, zone })
}
</script>

<template>
  <aside
    v-if="seat"
    class="flex w-20 shrink-0 flex-col gap-2 overflow-y-auto sm:w-24"
    aria-label="Your zones"
  >
    <DropdownMenu>
      <DropdownMenuTrigger as-child>
        <PlayZonePile
          zone="library"
          :count="seat.library_count"
          :game="table.store.game"
          hint="menu"
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        <DropdownMenuItem @select="table.draw(1)">Draw a card</DropdownMenuItem>
        <DropdownMenuItem @select="table.draw(7)">Draw seven</DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem @select="table.lookTop()">Look at the top…</DropdownMenuItem>
        <DropdownMenuItem @select="table.searchLibrary()">Search library…</DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem @select="table.shuffle()">Shuffle</DropdownMenuItem>
        <DropdownMenuItem @select="table.mulligan()">Mulligan…</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>

    <PlayZonePile
      zone="graveyard"
      :count="seat.graveyard.length"
      :top="graveyard"
      :game="table.store.game"
      :hint="graveyard ? cardName(graveyard) : null"
      @click="openZone('graveyard')"
    />
    <PlayZonePile
      zone="exile"
      :count="seat.exile.length"
      :top="exile"
      :game="table.store.game"
      @click="openZone('exile')"
    />

    <section v-if="showsCommandZone" data-play-drop="command" class="rounded-lg border p-1.5">
      <h3 class="text-muted-foreground mb-1 text-center text-[0.65rem]">Command</h3>
      <p v-if="commanders.length === 0" class="text-muted-foreground/70 text-center text-[0.6rem]">
        empty
      </p>
      <div class="space-y-1">
        <PlayCardMenu v-for="card in commanders" :key="card.id" :card="card" zone="command">
          <PlayCard
            :card="card"
            :game="table.store.game"
            size="small"
            @click="table.playCard(card)"
          />
        </PlayCardMenu>
      </div>
    </section>
  </aside>
</template>
