<script setup lang="ts">
import { computed } from 'vue'
import type { Card } from '@/lib/api'
import PriceStatGrid from '@/components/shared/PriceStatGrid.vue'
import { useCurrency } from '@/composables/useCurrency'

const props = defineProps<{ card: Card }>()

// The canonical USD prices converted for this viewer. Keep the direct Cardmarket EUR
// quote and MTGO tix alongside them: those are market feeds, not FX estimates.
const money = useCurrency()
const priceRows = computed(() =>
  [
    { label: money.displayCurrency.value, value: money.formatUsd(props.card.prices.usd) },
    {
      label: `${money.displayCurrency.value} foil`,
      value: money.formatUsd(props.card.prices.usd_foil),
    },
    { label: 'Cardmarket EUR', value: props.card.prices.eur ? `€${props.card.prices.eur}` : null },
    { label: 'MTGO tix', value: props.card.prices.tix ?? null },
  ].filter((row): row is { label: string; value: string } => row.value != null),
)
</script>

<template>
  <div v-if="priceRows.length">
    <h2 class="mb-2 text-sm font-semibold">Prices</h2>
    <PriceStatGrid :rows="priceRows" />
  </div>
</template>
