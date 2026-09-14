<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import PlayCard from '@/components/play/table/PlayCard.vue'
import PlayCardMenu from '@/components/play/table/PlayCardMenu.vue'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { fanLayout } from '@/lib/playTable'
import type { PlayCardView } from '@/lib/api/play'

// My hand, along the bottom edge.
//
// A hand is the one row on the table whose contents you cannot control: it is seven cards at
// the start and can be twenty by turn nine. So it **fans** — cards slide under each other by
// exactly as much as is needed and no more (`fanLayout`), down to a floor where the left edge
// of every card, and therefore its name, is still visible. Past that the row scrolls rather
// than compressing into an unreadable stack.
//
// Double-click plays a card to the middle of the board; dragging plays it where you dropped
// it. Both exist because both are habits people bring from other tables.
const table = usePlayTableContext()
const el = ref<HTMLElement | null>(null)
const width = ref(0)

/** How wide one card is drawn in the hand. Fixed, so the fan maths has a unit to work in. */
const CARD_WIDTH = 92

function measure() {
  width.value = el.value?.clientWidth ?? 0
}

onMounted(() => {
  measure()
  window.addEventListener('resize', measure)
})
onBeforeUnmount(() => window.removeEventListener('resize', measure))

const cards = computed<PlayCardView[]>(() => {
  const seatId = table.store.mySeatId
  return seatId === null ? [] : (table.store.cardsIn(seatId, 'hand') as PlayCardView[])
})

const fan = computed(() => fanLayout(cards.value.length, width.value || CARD_WIDTH, CARD_WIDTH))

function offset(index: number): string {
  return `${Math.round(index * fan.value.step)}px`
}

function onPointerDown(event: PointerEvent, card: PlayCardView) {
  table.startCardDrag(event, card, 'hand')
}

function onEnter(event: PointerEvent | FocusEvent, card: PlayCardView) {
  const target = event.currentTarget
  table.hoverCard(card, target instanceof HTMLElement ? target : null)
}
</script>

<template>
  <div
    ref="el"
    data-play-drop="hand"
    class="bg-muted/40 border-border/60 relative w-full overflow-x-auto rounded-t-lg border-t px-2 pt-2 pb-1"
    role="group"
    :aria-label="`Hand, ${cards.length} ${cards.length === 1 ? 'card' : 'cards'}`"
  >
    <p v-if="cards.length === 0" class="text-muted-foreground py-6 text-center text-xs">
      Your hand is empty.
    </p>
    <!-- The row is sized to the fan, not to the cards, because the cards are absolutely
      placed inside it — that is what lets them overlap without changing their own width. -->
    <div
      v-else
      class="relative mx-auto"
      :style="{
        width: `${Math.round(fan.width)}px`,
        height: `${Math.round((CARD_WIDTH * 85) / 61)}px`,
      }"
    >
      <div
        v-for="(card, index) in cards"
        :key="card.id"
        class="absolute top-0 transition-[left] duration-150 motion-reduce:transition-none hover:z-20"
        :style="{ left: offset(index), width: `${CARD_WIDTH}px`, zIndex: index }"
      >
        <PlayCardMenu :card="card" zone="hand">
          <PlayCard
            :card="card"
            :game="table.store.game"
            size="small"
            :selected="table.selectedId.value === card.id"
            :dragging="table.drag.value?.cardId === card.id && table.drag.value.moved"
            @pointerdown="(event: PointerEvent) => onPointerDown(event, card)"
            @dblclick="table.playCard(card)"
            @pointerenter="(event: PointerEvent) => onEnter(event, card)"
            @pointerleave="table.hoverCard(null, null)"
            @focus="(event: FocusEvent) => onEnter(event, card)"
            @blur="table.hoverCard(null, null)"
          />
        </PlayCardMenu>
      </div>
    </div>
  </div>
</template>
