<script setup lang="ts">
import { computed } from 'vue'
import type { Card } from '@/lib/api'
import PriceStatGrid from '@/components/shared/PriceStatGrid.vue'
import { useCurrency } from '@/composables/useCurrency'

const props = defineProps<{ card: Card }>()

// The canonical USD prices converted for this viewer — regular, foil and, where the printing
// comes etched, the etched-foil quote (issue #676). Keep the direct Cardmarket EUR quote and
// MTGO tix alongside them: those are market feeds, not FX estimates.
const money = useCurrency()
const priceRows = computed(() =>
  [
    { label: money.displayCurrency.value, value: money.formatUsd(props.card.prices.usd) },
    {
      label: `${money.displayCurrency.value} foil`,
      value: money.formatUsd(props.card.prices.usd_foil),
    },
    {
      label: `${money.displayCurrency.value} etched`,
      value: money.formatUsd(props.card.prices.usd_etched),
    },
    { label: 'Cardmarket EUR', value: props.card.prices.eur ? `€${props.card.prices.eur}` : null },
    { label: 'MTGO tix', value: props.card.prices.tix ?? null },
  ].filter((row): row is { label: string; value: string } => row.value != null),
)

// The etched price is a catalog quote, not a valuation of anything held: the collection and
// wish list have no etched bucket (holding lots, issue #594), so an etched copy is counted and
// valued as foil. Say so beside the number, so it isn't read as what a held copy is worth.
const showsEtched = computed(() => props.card.prices.usd_etched != null)
</script>

<template>
  <div v-if="priceRows.length">
    <h2 class="mb-2 text-sm font-semibold">Prices</h2>
    <PriceStatGrid :rows="priceRows" />
    <p v-if="showsEtched" class="text-muted-foreground mt-2 text-xs" data-testid="etched-note">
      Etched copies are held and valued as foil in your collection and wish list.
    </p>
  </div>
</template>
