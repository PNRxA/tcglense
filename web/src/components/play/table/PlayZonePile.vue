<script setup lang="ts">
import { ChevronDown } from '@lucide/vue'
import PlayCard from '@/components/play/table/PlayCard.vue'
import { zoneLabel } from '@/lib/playTable'
import type { PlayCardView, PlayZone } from '@/lib/api/play'

// One pile on the rail: a library, a graveyard, an exile.
//
// **A short tile, not a card.** The first version gave every pile a full 61:85 box, which read
// beautifully and then pushed the exile's label — and the whole command zone, commander and
// all — below the fold of a 1440×900 rail with nothing to say they were there. A pile's job
// on the rail is to answer "how many, and can I drop this on it"; the *contents* are one click
// away in the viewer. So the tile is a line: a thumb, the zone's name, its count.
//
// The thumb earns its place only where the picture is the information:
//
// - a **graveyard** shows its top card, because "what just died" is read across the table;
// - a **library** has nothing to show by definition, so it shows a card back — a face-down
//   stack is what it is;
// - an **empty** pile shows a dashed outline, which is the difference between "nothing in
//   here" and "something in here I'm not showing you".
//
// It stays a `<button>` with a real label: dragging a card onto the graveyard is the fast path
// and clicking it is the keyboard-and-screen-reader path, and `data-play-drop` is what the
// drag machine hit-tests for.
withDefaults(
  defineProps<{
    zone: PlayZone
    count: number
    /** The card to show on top, where seeing it is the point (the graveyard). */
    top?: PlayCardView | null
    game: string
    /** False for a pile you can only read (an opponent's). */
    droppable?: boolean
    /** Marks the tile as opening a menu rather than a viewer. */
    menu?: boolean
  }>(),
  { top: null, droppable: true, menu: false },
)
</script>

<template>
  <button
    type="button"
    :data-play-drop="droppable ? zone : undefined"
    class="hover:border-ring/60 hover:bg-accent/40 flex w-full items-center gap-1.5 rounded-lg border px-1.5 py-1 text-left transition-colors"
    :aria-label="`${zoneLabel(zone)}, ${count} ${count === 1 ? 'card' : 'cards'}`"
  >
    <!-- Fixed-width thumb column, so three tiles line their text up whatever they show. -->
    <span class="w-7 shrink-0">
      <PlayCard v-if="top" :card="top" :game="game" size="small" :interactive="false" />
      <span
        v-else-if="count > 0"
        class="bg-muted border-border/60 block aspect-[61/85] w-full rounded-sm border"
        aria-hidden="true"
      />
      <span
        v-else
        class="bg-muted/40 block aspect-[61/85] w-full rounded-sm border border-dashed"
        aria-hidden="true"
      />
    </span>
    <span class="min-w-0 flex-1">
      <span class="text-muted-foreground block truncate text-[0.65rem] leading-tight">
        {{ zoneLabel(zone) }}
      </span>
      <span class="block text-sm leading-tight font-semibold tabular-nums">{{ count }}</span>
    </span>
    <ChevronDown v-if="menu" class="text-muted-foreground/70 size-3 shrink-0" aria-hidden="true" />
  </button>
</template>
