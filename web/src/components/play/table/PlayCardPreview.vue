<script setup lang="ts">
import { computed } from 'vue'
import { X } from '@lucide/vue'
import PlayCard from '@/components/play/table/PlayCard.vue'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { cardName } from '@/lib/playTable'

// The card, big enough to read.
//
// A table renders cards at thumbnail size — a battlefield of fifteen permanents on a laptop
// gives each one about 80px — so "what does this actually say" has to be one hover away. The
// preview is a fixed overlay rather than a tooltip on the card because the card may be
// rotated, clipped by a scrolling strip, or half under another card; anchoring to the
// viewport instead sidesteps all three.
//
// Placement: beside the card it came from, flipped to whichever side has room, and clamped
// into the viewport — a preview that runs off the bottom of a phone is the same as no
// preview. A **pinned** preview ("view larger", or a long press on touch) also gets a close
// button and a backdrop, because on touch there is no "move the pointer away".
const table = usePlayTableContext()

const PREVIEW_WIDTH = 264
const PREVIEW_HEIGHT = Math.round((PREVIEW_WIDTH * 85) / 61)
const MARGIN = 8

const style = computed(() => {
  const preview = table.preview.value
  const width = typeof window === 'undefined' ? 1024 : window.innerWidth
  const height = typeof window === 'undefined' ? 768 : window.innerHeight
  if (!preview) return {}
  const anchor = preview.anchor
  const rightRoom = width - (anchor.left + anchor.width)
  // Prefer the side with room; a pinned preview opened from nothing centres itself.
  const left =
    anchor.width === 0
      ? Math.round((width - PREVIEW_WIDTH) / 2)
      : rightRoom > PREVIEW_WIDTH + MARGIN
        ? anchor.left + anchor.width + MARGIN
        : Math.max(MARGIN, anchor.left - PREVIEW_WIDTH - MARGIN)
  const top =
    anchor.height === 0
      ? Math.round((height - PREVIEW_HEIGHT) / 2)
      : Math.min(
          Math.max(MARGIN, anchor.top + anchor.height / 2 - PREVIEW_HEIGHT / 2),
          Math.max(MARGIN, height - PREVIEW_HEIGHT - MARGIN),
        )
  return {
    left: `${Math.min(Math.max(MARGIN, left), Math.max(MARGIN, width - PREVIEW_WIDTH - MARGIN))}px`,
    top: `${top}px`,
    width: `${PREVIEW_WIDTH}px`,
  }
})

const oracle = computed(() => {
  const card = table.preview.value?.card
  const def = card?.def
  if (!def || !card) return null
  return def.faces[card.face_index] ?? def.faces[0] ?? null
})
</script>

<template>
  <div v-if="table.preview.value" class="pointer-events-none">
    <!-- Only a pinned preview takes the screen; a hover preview must never swallow the
      pointer that is still moving across the board. -->
    <div
      v-if="table.preview.value.pinned"
      class="bg-background/60 pointer-events-auto fixed inset-0 z-40"
      @click="table.closePreview()"
    />
    <div
      class="fixed z-50 drop-shadow-xl"
      :class="table.preview.value.pinned ? 'pointer-events-auto' : ''"
      :style="style"
      role="dialog"
      :aria-label="cardName(table.preview.value.card)"
    >
      <PlayCard
        :card="{ ...table.preview.value.card, tapped: false, face_down: false }"
        :game="table.store.game"
        size="large"
        :interactive="false"
      />
      <div
        v-if="table.preview.value.pinned && oracle?.oracle_text"
        class="bg-popover text-popover-foreground mt-2 max-h-40 overflow-y-auto rounded-md border p-2 text-xs whitespace-pre-line"
      >
        {{ oracle.oracle_text }}
      </div>
      <button
        v-if="table.preview.value.pinned"
        type="button"
        class="bg-popover text-popover-foreground absolute -top-2 -right-2 grid size-7 place-items-center rounded-full border shadow-sm"
        aria-label="Close preview"
        @click="table.closePreview()"
      >
        <X class="size-4" />
      </button>
    </div>
  </div>
</template>
