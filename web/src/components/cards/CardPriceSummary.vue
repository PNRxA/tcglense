<script setup lang="ts">
import { computed } from 'vue'
import type { Card } from '@/lib/api'

const props = defineProps<{ card: Card }>()

// The card's current prices, formatted with their currency symbol; blank fields are
// dropped so the grid only shows prices we actually have.
const priceRows = computed(() => {
  const p = props.card.prices
  return [
    { label: 'USD', value: p.usd ? `$${p.usd}` : null },
    { label: 'USD foil', value: p.usd_foil ? `$${p.usd_foil}` : null },
    { label: 'EUR', value: p.eur ? `€${p.eur}` : null },
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
