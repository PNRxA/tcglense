<script setup lang="ts">
import type { DeckIssueStatus } from '@/lib/api'
import { DECK_ISSUE_TEXT_CLASS, deckIssueLabel } from '@/lib/legality'

// The strip of chips along the bottom edge of a deck card tile: how many copies of the card
// the deck holds (left) and what's notable about them (right) — a format-legality breach on
// the deck pages, "N foil" on a precon's. Shared by the owner, public and precon grids, which
// each pinned their own pair to the bottom two corners.
//
// Opposite corners only keep two chips apart while they both fit. They don't: the owner's
// control grows a second chip for a card held in both finishes, and on a small tile
// "Over Limit" landed on top of it. One flex line can't overlap itself — and this one wraps
// *upwards* (`flex-wrap-reverse` puts the first line at the bottom), so when the pair is
// wider than the tile the trailing chip lifts above the control instead of being clipped or
// truncated, and the control keeps the corner it has always had.
//
// The strip spans the tile, so it stays `pointer-events-none` for the stretched link
// underneath — only the control (a real popover trigger on the owner's grid) takes events
// back.
defineProps<{ legalityStatus?: DeckIssueStatus | null }>()
</script>

<template>
  <div
    class="pointer-events-none absolute inset-x-1.5 bottom-1.5 z-20 flex flex-wrap-reverse items-end justify-end gap-1"
  >
    <div class="pointer-events-auto mr-auto"><slot name="control" /></div>
    <slot name="trailing">
      <span
        v-if="legalityStatus"
        class="bg-background/90 max-w-full truncate rounded-md border px-1.5 py-0.5 text-xs font-medium shadow select-none"
        :class="DECK_ISSUE_TEXT_CLASS[legalityStatus]"
      >
        {{ deckIssueLabel(legalityStatus) }}
      </span>
    </slot>
  </div>
</template>
