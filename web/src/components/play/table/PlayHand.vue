<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import PlayCard from '@/components/play/table/PlayCard.vue'
import PlayCardMenu from '@/components/play/table/PlayCardMenu.vue'
import PlayHandActionBar from '@/components/play/table/PlayHandActionBar.vue'
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
//
// The fan is measured off the hand's **own box**, with a `ResizeObserver` rather than a window
// `resize` listener: most of what changes this row's width never resizes the window. Opening
// the log panel, the opponent strip gaining a row, a phone's keyboard appearing — each one
// takes hundreds of pixels off the hand while `window.innerWidth` sits still, and a fan
// computed for the old width either overflows its box or leaves half of it empty until
// something else happens to trigger a re-measure.
const table = usePlayTableContext()
const el = ref<HTMLElement | null>(null)
const width = ref(0)

/**
 * How wide one card is drawn in the hand — the unit the fan maths works in.
 *
 * A hand row is a card tall plus padding, so on a **short** viewport (a phone on its side, 390px
 * of height for everything) the cards shrink: at the full size the row alone was a quarter of
 * the screen and the battlefield got 2px.
 */
const FULL_CARD_WIDTH = 92
const SHORT_CARD_WIDTH = 54
const cardWidth = computed(() => (table.isShort.value ? SHORT_CARD_WIDTH : FULL_CARD_WIDTH))

function measure() {
  width.value = el.value?.clientWidth ?? 0
}

let observer: ResizeObserver | null = null

onMounted(() => {
  measure()
  if (typeof ResizeObserver !== 'undefined' && el.value) {
    observer = new ResizeObserver(measure)
    observer.observe(el.value)
    return
  }
  // No ResizeObserver (an old browser, or a test environment): the window is the only signal
  // left, and a hand that re-fans on rotation is better than one that never re-fans at all.
  window.addEventListener('resize', measure)
})

onBeforeUnmount(() => {
  observer?.disconnect()
  observer = null
  window.removeEventListener('resize', measure)
})

const cards = computed<PlayCardView[]>(() => {
  const seatId = table.store.mySeatId
  return seatId === null ? [] : (table.store.cardsIn(seatId, 'hand') as PlayCardView[])
})

const fan = computed(() =>
  fanLayout(cards.value.length, width.value || cardWidth.value, cardWidth.value),
)

function offset(index: number): string {
  return `${Math.round(index * fan.value.step)}px`
}

function onPointerDown(event: PointerEvent, card: PlayCardView) {
  table.startCardDrag(event, card, 'hand')
}

function onEnter(event: PointerEvent | FocusEvent, card: PlayCardView) {
  const target = event.currentTarget
  const pointerType = 'pointerType' in event ? event.pointerType : undefined
  table.hoverCard(card, target instanceof HTMLElement ? target : null, pointerType)
}

function onLeave(event: PointerEvent) {
  table.hoverCard(null, null, event.pointerType)
}
</script>

<template>
  <!-- Two boxes on purpose. The inner one scrolls (a fanned hand can be wider than the
    screen), and an `overflow-x-auto` box clips its children on *both* axes — so the action
    bar, which sits above the row's top edge, has to live in the outer one or it is invisible
    exactly when it matters. -->
  <div class="relative w-full shrink-0">
    <PlayHandActionBar />

    <div
      ref="el"
      data-play-drop="hand"
      class="bg-muted/40 border-border/60 w-full overflow-x-auto rounded-t-lg border-t px-2"
      :class="table.isShort.value ? 'pt-1 pb-0.5' : 'pt-2 pb-1'"
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
          height: `${Math.round((cardWidth * 85) / 61)}px`,
        }"
      >
        <div
          v-for="(card, index) in cards"
          :key="card.id"
          class="absolute top-0 transition-[left] duration-150 motion-reduce:transition-none hover:z-20"
          :style="{ left: offset(index), width: `${cardWidth}px`, zIndex: index }"
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
              @pointerleave="(event: PointerEvent) => onLeave(event)"
              @focus="(event: FocusEvent) => onEnter(event, card)"
              @blur="table.hoverCard(null, null)"
            />
          </PlayCardMenu>
        </div>
      </div>
    </div>
  </div>
</template>
