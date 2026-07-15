<script setup lang="ts">
import { computed } from 'vue'
import type { Card } from '@/lib/api'
import { useCurrency } from '@/composables/useCurrency'

const props = defineProps<{ card: Card }>()

// The canonical USD prices converted for this viewer. Keep the direct Cardmarket EUR
// quote and MTGO tix alongside them: those are market feeds, not FX estimates.
const money = useCurrency()
const priceRows = computed(() => {
  const p = props.card.prices
  return [
    { label: money.displayCurrency.value, value: money.formatUsd(p.usd) },
    { label: `${money.displayCurrency.value} foil`, value: money.formatUsd(p.usd_foil) },
    { label: 'Cardmarket EUR', value: p.eur ? `€${p.eur}` : null },
    { label: 'MTGO tix', value: p.tix ?? null },
  ].filter((row) => row.value)
})
</script>

<template>
  <div v-if="priceRows.length" class="mt-6">
    <h2 class="mb-2 text-sm font-semibold">Prices</h2>
    <dl class="grid grid-cols-2 gap-2 sm:grid-cols-4">
      <div v-for="row in priceRows" :key="row.label" class="bg-muted/50 rounded-lg border p-3">
        <dt class="text-muted-foreground text-xs">{{ row.label }}</dt>
        <dd class="mt-0.5 font-medium tabular-nums">{{ row.value }}</dd>
      </div>
    </dl>
  </div>
</template>
