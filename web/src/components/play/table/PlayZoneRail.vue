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
import type { PlayCardView } from '@/lib/api/play'

// My own zones, as a rail down the left of the board.
//
// **All four are visible at once, or the rail has failed.** A zone you have to scroll a
// 96px-wide column to find is a zone you forget you have — the first cut of this rail stacked
// four full-size card boxes and pushed the command zone (the commander!) off the bottom at
// 1440×900. So the three ordinary piles are short tiles (`PlayZonePile`) and only the command
// zone spends real height on pictures. Budget, at the rail's own width: three tiles at ~48px,
// the command section at ~110px, gaps — roughly 270px, against ~360px of rail at 1280×720.
// `overflow-y-auto` survives as a last resort for the case that genuinely overflows it: a pair
// of partner commanders on a short viewport (which is also why two commanders go side by side
// rather than stacked).
//
// **Command comes first** because in Commander it is the pile you touch most — it opens the
// game and it is where the commander goes back to, every time.
//
// The library is the only pile with a menu rather than an action, because it is the only one
// with more than one obvious thing to do to it — draw, look, search, shuffle, mulligan — and
// every one of those is a decision you make deliberately. The other two open the viewer, which
// is what "look through my graveyard" means.
const table = usePlayTableContext()

const seat = computed(() => table.store.mySeat)
const seatId = computed(() => table.store.mySeatId)

/** The card a pile shows on its tile — the top of the zone, in the zone's stored order. */
function pileTop(zone: 'graveyard' | 'exile'): PlayCardView | null {
  const id = seatId.value
  if (id === null) return null
  return (table.store.cardsIn(id, zone) as PlayCardView[])[0] ?? null
}

const graveyard = computed(() => pileTop('graveyard'))
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

function hover(event: PointerEvent | FocusEvent, card: PlayCardView | null) {
  const target = event.currentTarget
  table.hoverCard(card, target instanceof HTMLElement ? target : null)
}
</script>

<template>
  <aside
    v-if="seat"
    class="flex w-24 shrink-0 flex-col gap-1.5 overflow-y-auto sm:w-28"
    aria-label="Your zones"
  >
    <!-- The command zone: the one pile on the rail worth a picture, because the card in it is
      the deck. Two commanders sit side by side so a partner pair costs no extra height. -->
    <section
      v-if="showsCommandZone"
      data-play-drop="command"
      class="shrink-0 rounded-lg border px-1.5 py-1"
      aria-label="Command zone"
    >
      <h3 class="text-muted-foreground mb-1 text-[0.65rem] leading-tight">
        Command
        <span class="text-foreground font-semibold tabular-nums">{{ commanders.length }}</span>
      </h3>
      <p v-if="commanders.length === 0" class="text-muted-foreground/70 py-1 text-[0.6rem]">
        Nothing here.
      </p>
      <div
        v-else
        class="grid gap-1"
        :class="commanders.length > 1 ? 'grid-cols-2' : 'grid-cols-1 px-2'"
      >
        <PlayCardMenu v-for="card in commanders" :key="card.id" :card="card" zone="command">
          <PlayCard
            :card="card"
            :game="table.store.game"
            size="small"
            @click="table.playCard(card)"
            @pointerenter="(event: PointerEvent) => hover(event, card)"
            @pointerleave="(event: PointerEvent) => hover(event, null)"
            @focus="(event: FocusEvent) => hover(event, card)"
            @blur="(event: FocusEvent) => hover(event, null)"
          />
        </PlayCardMenu>
      </div>
    </section>

    <DropdownMenu>
      <DropdownMenuTrigger as-child>
        <PlayZonePile
          zone="library"
          :count="seat.library_count"
          :game="table.store.game"
          menu
          class="shrink-0"
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
      class="shrink-0"
      @click="openZone('graveyard')"
    />
    <PlayZonePile
      zone="exile"
      :count="seat.exile.length"
      :game="table.store.game"
      class="shrink-0"
      @click="openZone('exile')"
    />
  </aside>
</template>
