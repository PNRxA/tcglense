<script setup lang="ts">
import PlayCard from '@/components/play/table/PlayCard.vue'
import { zoneLabel } from '@/lib/playTable'
import type { PlayCardView, PlayZone } from '@/lib/api/play'

// One pile on the rail: a library, a graveyard, an exile.
//
// A pile shows its **top card** where it has one — a graveyard whose top card is visible is
// how everybody at a paper table knows what just died — and its count always, because the
// count is the thing you read across the table. A library has no top card to show by
// definition, so it is a back with a number on it.
//
// It is also a drop target (`data-play-drop`), which is the reason it is a `<button>` with a
// real label rather than a decorative div: dragging a card onto the graveyard is the fast
// path, clicking it is the keyboard-and-screen-reader path, and both have to exist.
withDefaults(
  defineProps<{
    zone: PlayZone
    count: number
    /** The card to show on top, if this pile shows one. */
    top?: PlayCardView | null
    game: string
    /** False for a pile you can only read (an opponent's). */
    droppable?: boolean
    /** A short line under the count — "top card", a seat's name, a reminder. */
    hint?: string | null
  }>(),
  { top: null, droppable: true, hint: null },
)
</script>

<template>
  <button
    type="button"
    :data-play-drop="droppable ? zone : undefined"
    class="hover:border-ring/60 hover:bg-accent/40 flex w-full flex-col items-center gap-1 rounded-lg border p-1.5 transition-colors"
    :aria-label="`${zoneLabel(zone)}, ${count} ${count === 1 ? 'card' : 'cards'}`"
  >
    <div class="relative w-full">
      <PlayCard v-if="top" :card="top" :game="game" size="small" :interactive="false" />
      <!-- An empty pile keeps the card-shaped hole rather than collapsing: the rail must not
        reflow every time a graveyard empties. -->
      <div
        v-else
        class="bg-muted/60 text-muted-foreground/60 grid aspect-[61/85] w-full place-items-center rounded-[4.76%_/_3.42%] border border-dashed text-[0.6rem]"
      >
        {{ count > 0 ? count : 'empty' }}
      </div>
    </div>
    <span class="text-muted-foreground w-full truncate text-center text-[0.65rem] leading-tight">
      {{ zoneLabel(zone) }}
      <span class="text-foreground font-semibold tabular-nums">{{ count }}</span>
    </span>
    <span v-if="hint" class="text-muted-foreground/80 w-full truncate text-center text-[0.6rem]">
      {{ hint }}
    </span>
  </button>
</template>
