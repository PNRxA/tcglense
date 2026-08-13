<script setup lang="ts">
import { computed } from 'vue'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'
import { useCurrency } from '@/composables/useCurrency'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import type { DeckCardEntry, DeckIssueStatus } from '@/lib/api'
import { displayUsdPrice } from '@/lib/cardPrice'
import { DECK_ISSUE_TEXT_CLASS, deckIssueLabel } from '@/lib/legality'
import { displayManaCost } from '@/lib/mana'

// One card as a compact row — the "list" deck view (issue #570). The image grid is the
// right shape for building a deck; this is the right shape for *reading* one: a 100-card
// list fits on a screen or two, and the facts you scan for (mana cost, type, price) are in
// aligned columns instead of buried in art.
//
// Deliberately layout-only. Everything owner-specific arrives through slots — `#control`
// (the quantity editor, or a static ×N on the public view) and `#badges` (collection /
// wish-list counts) — so the owner and public views share this row exactly as they share
// CardTile, with no `readonly` flag to thread through.
const props = defineProps<{
  game: string
  entry: DeckCardEntry
  legalityStatus?: DeckIssueStatus | null
}>()

const money = useCurrency()
const card = computed(() => props.entry.card)

// Same navigation contract as CardTile: a plain left-click opens the shared detail modal
// over this page (the deck list keeps its scroll and filter state), while the href stays
// the real card page so modifier/middle clicks and crawlers get the full document.
const { hrefFor, onActivate, warm } = useDetailModalLink()
const href = computed(() => hrefFor('card', props.game, card.value.id))
function onClick(event: MouseEvent) {
  onActivate(event, 'card', props.game, card.value.id)
}

const price = computed(() => {
  const picked = displayUsdPrice(card.value.prices)
  return picked ? { ...picked, text: money.formatUsd(picked.amount) } : null
})

// The front face's types only — a modal DFC's back half would double the column's width
// for no extra information at this density.
const typeLine = computed(() => card.value.type_line?.split('//')[0]?.trim() ?? '')

// The cost goes through the shared seam, because a transforming card has no top-level
// `mana_cost` at all and reading that field directly left this column empty for every one
// of them. Note it is *not* truncated to the front half the way the type line above is: a
// split or adventure card's printed cost is the combined top-level one, and the seam hands
// it back whole.
const manaCost = computed(() => displayManaCost(card.value))
</script>

<template>
  <div
    class="hover:bg-muted/50 flex items-center gap-2 rounded-md px-1.5 py-1 transition-colors sm:gap-3"
  >
    <!-- The control column reserves the width of the *pair* of count chips the owner's
      control renders for a card held in both finishes (total + foil), at every breakpoint.
      It used to be fixed at a single chip's width, which the pair overflowed — the chips
      painted over the card name — and sizing it to whichever chips a row happens to carry
      would make the name column start in a different place on every foil row. Names line
      up down the list either way, which is the whole point of the compact view.
      It's a floor, not a cap, so a three-digit count widens the cell rather than spilling
      out of it; a deck holding 100+ copies of one card shifts that row's name and no
      other. -->
    <div class="flex min-w-18 shrink-0 justify-start"><slot name="control" /></div>

    <a
      :href="href"
      class="min-w-0 flex-1 truncate text-sm font-medium hover:underline"
      :title="card.name"
      @click="onClick"
      @pointerenter="warm('card')"
      @focusin="warm('card')"
      >{{ card.name }}</a
    >

    <ManaSymbols v-if="manaCost" :text="manaCost" class="hidden shrink-0 text-xs sm:inline" />

    <span class="text-muted-foreground hidden w-52 shrink-0 truncate text-xs lg:inline">{{
      typeLine
    }}</span>

    <span
      v-if="legalityStatus"
      class="shrink-0 text-xs font-medium"
      :class="DECK_ISSUE_TEXT_CLASS[legalityStatus]"
      >{{ deckIssueLabel(legalityStatus) }}</span
    >

    <slot name="badges" />

    <span class="text-muted-foreground hidden w-24 shrink-0 truncate text-right text-xs sm:inline"
      >{{ card.set_code.toUpperCase() }} · #{{ card.collector_number }}</span
    >

    <span class="text-muted-foreground w-16 shrink-0 text-right text-xs tabular-nums">{{
      price?.text ?? '—'
    }}</span>
  </div>
</template>
