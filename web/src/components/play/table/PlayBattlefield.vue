<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import PlayCard from '@/components/play/table/PlayCard.vue'
import PlayCardMenu from '@/components/play/table/PlayCardMenu.vue'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { battlefieldLayout } from '@/lib/playTable'
import type { PlayCardView } from '@/lib/api/play'

// One seat's board.
//
// Cards are placed by the `x`/`y` **fractions** the wire carries, never by pixels: the same
// board is drawn full-size in front of its owner and thumbnail-size in everyone else's strip,
// and a fraction is the only placement that survives both. A card's fraction is its *centre*
// (hence the `-50%` translate), which is also what `pointToFraction` returns for a pointer —
// so a card lands exactly under the finger that dropped it.
//
// `data-play-drop` is the whole drop-target protocol: the drag machine asks the document what
// is under the pointer and reads this attribute off it. Adding a new drop target anywhere on
// the table is one attribute, not a registration.
const props = withDefaults(
  defineProps<{
    seatId: number
    /** False for an opponent's board: it is watched, not operated. */
    interactive?: boolean
    /** A card's width as a percentage of the board, so boards scale as one piece. */
    cardWidth?: string
    /**
     * Whether cards carry their context menu. Defaults to {@link interactive} — but an
     * opponent's board sets it on regardless, because "take control of that" is reached from
     * the card it is about, and that card is on their side of the table.
     */
    menu?: boolean
  }>(),
  { interactive: true, cardWidth: '9.5%', menu: undefined },
)

const table = usePlayTableContext()
const el = ref<HTMLElement | null>(null)

// Only my own board defines the coordinate space drops resolve against. An opponent's
// thumbnail board never registers *and never clears* — a seat joining mid-game remounts those
// boards, and a naive `setBattlefield(null)` there would quietly unregister mine.
watch(
  () => [el.value, props.interactive] as const,
  ([node, interactive]) => {
    if (interactive) table.setBattlefield(node)
  },
  { immediate: true },
)
onBeforeUnmount(() => {
  if (props.interactive) table.setBattlefield(null)
})

const showMenu = computed(() => props.menu ?? props.interactive)

const items = computed(() =>
  battlefieldLayout(table.store.cardsIn(props.seatId, 'battlefield') as PlayCardView[]),
)

function onPointerDown(event: PointerEvent, card: PlayCardView) {
  if (!props.interactive) return
  table.startCardDrag(event, card, 'battlefield')
}

function onEnter(event: PointerEvent, card: PlayCardView) {
  const target = event.currentTarget
  table.hoverCard(card, target instanceof HTMLElement ? target : null)
}

function onFocus(event: FocusEvent, card: PlayCardView) {
  const target = event.currentTarget
  table.hoverCard(card, target instanceof HTMLElement ? target : null)
}
</script>

<template>
  <div
    ref="el"
    :data-play-drop="interactive ? 'battlefield' : undefined"
    :data-play-seat="seatId"
    class="border-border/60 relative h-full w-full overflow-hidden rounded-lg border"
    :class="
      interactive
        ? 'bg-card [background-image:linear-gradient(to_right,var(--color-border)_1px,transparent_1px),linear-gradient(to_bottom,var(--color-border)_1px,transparent_1px)] [background-size:6.25%_12.5%] [background-blend-mode:soft-light]'
        : 'bg-muted/40'
    "
    role="group"
    aria-label="Battlefield"
  >
    <p
      v-if="items.length === 0"
      class="text-muted-foreground pointer-events-none absolute inset-0 grid place-items-center text-xs"
    >
      {{ interactive ? 'Drag a card here to play it' : 'Nothing on the battlefield' }}
    </p>
    <div
      v-for="item in items"
      :key="item.card.id"
      class="absolute"
      :style="{
        left: `${item.x * 100}%`,
        top: `${item.y * 100}%`,
        width: cardWidth,
        zIndex: item.z,
        transform: 'translate(-50%, -50%)',
      }"
    >
      <PlayCardMenu v-if="showMenu" :card="item.card" zone="battlefield">
        <PlayCard
          :card="item.card"
          :game="table.store.game"
          size="small"
          :selected="table.selectedId.value === item.card.id"
          :dragging="table.drag.value?.cardId === item.card.id && table.drag.value.moved"
          @pointerdown="(event: PointerEvent) => onPointerDown(event, item.card)"
          @pointerenter="(event: PointerEvent) => onEnter(event, item.card)"
          @pointerleave="table.hoverCard(null, null)"
          @focus="(event: FocusEvent) => onFocus(event, item.card)"
          @blur="table.hoverCard(null, null)"
        />
      </PlayCardMenu>
      <PlayCard
        v-else
        :card="item.card"
        :game="table.store.game"
        size="small"
        :interactive="false"
        @pointerenter="(event: PointerEvent) => onEnter(event, item.card)"
        @pointerleave="table.hoverCard(null, null)"
      />
    </div>
  </div>
</template>
