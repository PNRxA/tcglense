<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { RouterLink } from 'vue-router'
import { CircleCheck, ShoppingCart } from '@lucide/vue'
import { useCurrency } from '@/composables/useCurrency'
import { useNeededCardsQuery } from '@/composables/useDecks'
import { neededCostLine } from '@/lib/neededCost'
import type { NeedMode } from '@/lib/api'

// "To finish: 12 cards · ~$41" under a deck's name (issue #675) — what the owner still has
// to buy for *this* deck, priced, linking to the shopping list pre-filtered to it.
//
// It asks the API for this deck only (`?deck_id=`): the number is the deck's share of the
// shortfall across every deck, so a copy two decks share is not claimed as owned by both.
// Matched by card (any printing you own counts), the default the shopping list opens on, so
// the count here and the count there agree.
//
// Two honest silences: nothing renders until the answer lands (a header line that flickers
// from "fully owned" to "12 cards" is worse than one that appears), and a deck with nothing
// missing says so briefly rather than vanishing — "you own every card" is the good news a
// collector opened the page for, and the one state that distinguishes "nothing to buy" from
// "still working it out".
const props = defineProps<{ game: string; deckId: number }>()

const money = useCurrency()
const mode = ref<NeedMode>('card')
const { data } = useNeededCardsQuery(toRef(props, 'game'), mode, toRef(props, 'deckId'))

const totals = computed(() => data.value?.totals ?? null)
const line = computed(() => (totals.value ? neededCostLine(totals.value, money.formatUsd) : ''))
const listPath = computed(() => ({
  path: `/decks/${props.game}/needed`,
  query: { deck: String(props.deckId) },
}))
</script>

<template>
  <p v-if="totals" class="text-muted-foreground flex flex-wrap items-center gap-1.5 text-sm">
    <template v-if="totals.cards === 0">
      <CircleCheck class="text-success size-3.5 shrink-0" aria-hidden="true" />
      <span>Every card is in your collection</span>
    </template>
    <template v-else>
      <ShoppingCart class="size-3.5 shrink-0" aria-hidden="true" />
      <span class="tabular-nums">To finish: {{ line }}</span>
      <span aria-hidden="true">·</span>
      <RouterLink :to="listPath" class="hover:text-foreground shrink-0 hover:underline">
        Shopping list
      </RouterLink>
    </template>
  </p>
</template>
