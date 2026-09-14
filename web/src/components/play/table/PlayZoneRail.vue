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
// four full-size card boxes and pushed the exile's label and the whole command zone (the
// commander!) below the fold at 1440×900, with nothing on screen to say they were there.
//
// Two things fix it, and the second is the one that matters:
//
// 1. The three ordinary piles are **short tiles** (`PlayZonePile`) — a thumb, a name, a count.
//    A pile's job here is "how many, and can I drop on it"; its contents are one click away.
// 2. The command zone **takes the height that is left and scales its card to fit**, rather than
//    claiming a fixed 61:85 box and pushing whatever follows off the bottom. The card's width
//    is capped in container-query height units (`cqh`) against the section's own box, so on a
//    tall window the commander is large and on a 720p one it is small — and in neither case
//    does anything below it move. The `max-h` is the other half of that: past the point where
//    the card is already as wide as the rail, more height would only add void, so the section
//    stops growing and the slack falls below the exile instead. Measured in the running app,
//    nothing is clipped and nothing scrolls at either 1440×900 or 1280×720.
//
// `overflow-y-auto` stays as the genuine last resort (an absurdly short window), but in the
// normal run the content now fits by construction and it never engages.
//
// **Command comes first** because in Commander it is the pile you touch most — it opens the
// game and it is where the commander goes back to, every time.
//
// On a **phone** the same four zones turn ninety degrees: a row of four tiles directly above
// the hand, which is where your thumb already is, and which costs the battlefield no width at
// all. Same tiles, same `data-play-drop` targets, same library menu — only the axis changes,
// so a card dragged onto "Graveyard" lands in the graveyard either way.
//
// The library is the only pile with a menu rather than an action, because it is the only one
// with more than one obvious thing to do to it — draw, look, search, shuffle, mulligan — and
// every one of those is a decision you make deliberately. The other two open the viewer, which
// is what "look through my graveyard" means.
const table = usePlayTableContext()

/**
 * A card is 61 wide for every 85 tall, so the widest it may be drawn inside a box `H` tall is
 * `0.7176 × H` — expressed here in container-query height units against the command zone's own
 * box. Whichever of that and the rail's width is smaller wins, so the card fits both ways.
 */
const COMMANDER_SOLO_WIDTH = 'w-[min(100%,71.7cqh)]'
/** Partners sit side by side, so a pair costs the rail no more height than one commander. */
const COMMANDER_PAIR_WIDTH = 'w-[min(48%,71.7cqh)]'

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

function openZone(zone: 'graveyard' | 'exile' | 'command') {
  if (seatId.value === null) return
  table.openViewer({ kind: 'zone', seatId: seatId.value, zone })
}

function hover(event: PointerEvent | FocusEvent, card: PlayCardView | null) {
  const target = event.currentTarget
  const pointerType = 'pointerType' in event ? event.pointerType : undefined
  table.hoverCard(card, target instanceof HTMLElement ? target : null, pointerType)
}
</script>

<template>
  <!-- The phone's rail: four tiles across, above the hand. -->
  <div
    v-if="seat && table.compact.value"
    class="grid shrink-0 grid-cols-4 gap-1"
    aria-label="Your zones"
  >
    <!-- Dense on both orientations: at 390px a quarter of the width is ~92px, and the tile has
      to fit "Graveyard" beside a thumb without truncating the one word that identifies it. -->
    <PlayZonePile
      v-if="showsCommandZone"
      zone="command"
      label="Command"
      :count="commanders.length"
      :top="commanders[0] ?? null"
      :game="table.store.game"
      dense
      @click="openZone('command')"
    />
    <DropdownMenu>
      <DropdownMenuTrigger as-child>
        <PlayZonePile
          zone="library"
          label="Library"
          :count="seat.library_count"
          :game="table.store.game"
          dense
          menu
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" side="top">
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
      label="Graveyard"
      :count="seat.graveyard.length"
      :top="graveyard"
      :game="table.store.game"
      dense
      @click="openZone('graveyard')"
    />
    <PlayZonePile
      zone="exile"
      label="Exile"
      :count="seat.exile.length"
      :game="table.store.game"
      dense
      @click="openZone('exile')"
    />
  </div>

  <aside
    v-else-if="seat"
    class="flex w-24 shrink-0 flex-col gap-1.5 overflow-y-auto sm:w-28"
    aria-label="Your zones"
  >
    <!-- The command zone: the one pile on the rail worth a picture, because the card in it is
      the deck. Two commanders sit side by side so a partner pair costs no extra height. -->
    <section
      v-if="showsCommandZone"
      data-play-drop="command"
      class="flex flex-col rounded-lg border px-1.5 py-1"
      :class="commanders.length > 0 ? 'max-h-44 min-h-0 flex-1' : 'shrink-0'"
      aria-label="Command zone"
    >
      <h3 class="text-muted-foreground shrink-0 text-[0.65rem] leading-tight">
        Command
        <span class="text-foreground font-semibold tabular-nums">{{ commanders.length }}</span>
      </h3>
      <p v-if="commanders.length === 0" class="text-muted-foreground/70 py-1 text-[0.6rem]">
        Nothing here.
      </p>
      <!-- A size container, so the card below can be capped against the height that is
           actually left rather than against a number someone guessed. -->
      <div
        v-else
        class="mt-1 flex min-h-0 flex-1 justify-center gap-1"
        :style="{ containerType: 'size' }"
      >
        <div
          v-for="card in commanders"
          :key="card.id"
          :class="commanders.length > 1 ? COMMANDER_PAIR_WIDTH : COMMANDER_SOLO_WIDTH"
        >
          <PlayCardMenu :card="card" zone="command">
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
