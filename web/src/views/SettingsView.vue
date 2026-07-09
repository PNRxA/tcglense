<script setup lang="ts">
import { computed } from 'vue'
import { LayoutGrid, Wallet } from '@lucide/vue'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  NumberField,
  NumberFieldContent,
  NumberFieldDecrement,
  NumberFieldIncrement,
  NumberFieldInput,
} from '@/components/ui/number-field'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import {
  MAX_BULK_THRESHOLD_CENTS,
  MIN_BULK_THRESHOLD_CENTS,
  centsToDollars,
  dollarsToCents,
} from '@/lib/bulkThreshold'
import { CARD_SIZE_OPTIONS, isCardSize } from '@/lib/cardSize'
import { usePageMeta } from '@/lib/seo'
import { useBulkThresholdStore } from '@/stores/bulkThreshold'
import { useCardSizeStore } from '@/stores/cardSize'

// App-only preferences page, so keep it out of search indexes (like the profile page).
usePageMeta({ title: 'Settings', canonicalPath: '/settings', noindex: true })

const cardSize = useCardSizeStore()
const bulkThreshold = useBulkThresholdStore()

// The single-select toggle group hands back the chosen value, or '' when the active item
// is clicked again. Ignore the empty case so a size is always selected (the group is
// controlled, so not committing simply leaves the current choice), and narrow the string
// back to a CardSize on commit.
function onCardSize(value: unknown) {
  if (typeof value === 'string' && isCardSize(value)) cardSize.setSize(value)
}

// The number field edits dollars; the store holds whole cents. Round-trip through the
// bulk-threshold helpers so a cleared / sub-cent / out-of-range entry always resolves to
// a valid, clamped cents value.
const thresholdDollars = computed({
  get: () => centsToDollars(bulkThreshold.cents),
  set: (dollars: number) => bulkThreshold.setCents(dollarsToCents(dollars)),
})

const minDollars = centsToDollars(MIN_BULK_THRESHOLD_CENTS)
const maxDollars = centsToDollars(MAX_BULK_THRESHOLD_CENTS)
</script>

<template>
  <div class="mx-auto max-w-2xl px-4 py-12">
    <div class="mb-8">
      <h1 class="text-3xl font-semibold tracking-tight">Settings</h1>
      <p class="text-muted-foreground mt-2">Display preferences saved on this device.</p>
    </div>

    <div class="grid gap-6">
      <!-- Card size — surfaces the same preference as the in-toolbar size menu. -->
      <Card>
        <CardHeader>
          <CardTitle class="flex items-center gap-2 text-lg">
            <LayoutGrid class="size-5" /> Card size
          </CardTitle>
          <CardDescription>
            How large cards appear in the browse and collection grids. Smaller sizes fit more cards
            per row.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ToggleGroup
            type="single"
            variant="outline"
            :model-value="cardSize.size"
            aria-label="Card size"
            @update:model-value="onCardSize"
          >
            <ToggleGroupItem
              v-for="option in CARD_SIZE_OPTIONS"
              :key="option.value"
              :value="option.value"
            >
              {{ option.label }}
            </ToggleGroupItem>
          </ToggleGroup>
        </CardContent>
      </Card>

      <!-- Bulk threshold — a per-request cutoff sent to the collection value endpoints. -->
      <Card>
        <CardHeader>
          <CardTitle class="flex items-center gap-2 text-lg">
            <Wallet class="size-5" /> Bulk threshold
          </CardTitle>
          <CardDescription>
            Cards worth less than this each count as “bulk” in your collection's value — the
            low-value commons and uncommons, shown as a separate slice of the total.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <NumberField
            v-model="thresholdDollars"
            :min="minDollars"
            :max="maxDollars"
            :step="0.25"
            :step-snapping="false"
            :format-options="{
              style: 'currency',
              currency: 'USD',
              currencyDisplay: 'narrowSymbol',
            }"
            class="max-w-[12rem]"
          >
            <NumberFieldContent>
              <NumberFieldDecrement />
              <NumberFieldInput aria-label="Bulk threshold in US dollars" />
              <NumberFieldIncrement />
            </NumberFieldContent>
          </NumberField>
        </CardContent>
      </Card>
    </div>
  </div>
</template>
