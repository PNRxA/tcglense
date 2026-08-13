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
// The left corner is a *column* for the same reason: the owner's ownership chips ("you own
// N / want N") stack directly above the deck count they qualify rather than taking the
// tile's top-right corner. Both answer "how many?", so reading them together beats a
// diagonal scan — and the strip is the one edge a tile has already given up, so the art
// keeps the rest.
//
// The strip spans the tile, so it stays `pointer-events-none` for the stretched link
// underneath — only that column (a real popover trigger on the owner's grid, and the
// ownership chips' own tooltips) takes events back.
defineProps<{ legalityStatus?: DeckIssueStatus | null }>()
</script>

<template>
  <div
    class="pointer-events-none absolute inset-x-1.5 bottom-1.5 z-20 flex flex-wrap-reverse items-end justify-end gap-1"
  >
    <!-- One column, not two stacked wrappers: an empty `#ownership` slot renders no element
      at all, so the `gap` never opens above a card you don't own and the control keeps the
      exact corner it has on every other grid. `pointer-events` inherits, so both slots take
      their clicks back from the strip. -->
    <div class="pointer-events-auto mr-auto flex flex-col items-start gap-1">
      <slot name="ownership" />
      <slot name="control" />
    </div>
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
