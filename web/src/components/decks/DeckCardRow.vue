<script setup lang="ts">
import { computed } from 'vue'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'
import { useCurrency } from '@/composables/useCurrency'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import type { DeckCardEntry } from '@/lib/api'
import { displayUsdPrice } from '@/lib/cardPrice'
import { DECK_ISSUE_TEXT_CLASS, deckIssueLabel, type DeckIssueStatus } from '@/lib/legality'

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
</script>

<template>
  <div
    class="hover:bg-muted/50 flex items-center gap-2 rounded-md px-1.5 py-1 transition-colors sm:gap-3"
  >
    <div class="flex w-12 shrink-0 justify-start sm:w-14"><slot name="control" /></div>

    <a
      :href="href"
      class="min-w-0 flex-1 truncate text-sm font-medium hover:underline"
      :title="card.name"
      @click="onClick"
      @pointerenter="warm('card')"
      @focusin="warm('card')"
      >{{ card.name }}</a
    >

    <ManaSymbols
      v-if="card.mana_cost"
      :text="card.mana_cost"
      class="hidden shrink-0 text-xs sm:inline"
    />

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
