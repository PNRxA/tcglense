<script setup lang="ts">
import { computed, ref } from 'vue'
import { Check, CircleDollarSign, LayoutGrid, Palette, Wallet } from '@lucide/vue'
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useSetAccentMutation } from '@/composables/useAccent'
import { useSetCurrencyMutation } from '@/composables/useCurrency'
import { ApiError } from '@/lib/api'
import { ACCENT_OPTIONS, isAccent } from '@/lib/accent'
import {
  MAX_BULK_THRESHOLD_CENTS,
  MIN_BULK_THRESHOLD_CENTS,
  centsToDollars,
  dollarsToCents,
} from '@/lib/bulkThreshold'
import { CARD_SIZE_OPTIONS, isCardSize } from '@/lib/cardSize'
import { CURRENCY_OPTIONS, isSupportedCurrency } from '@/lib/currency'
import { usePageMeta } from '@/lib/seo'
import { useAccentStore } from '@/stores/accent'
import { useBulkThresholdStore } from '@/stores/bulkThreshold'
import { useCardSizeStore } from '@/stores/cardSize'
import { useAuthStore } from '@/stores/auth'

// App-only preferences page, so keep it out of search indexes (like the profile page).
usePageMeta({ title: 'Settings', canonicalPath: '/settings', noindex: true })

const cardSize = useCardSizeStore()
const bulkThreshold = useBulkThresholdStore()
const auth = useAuthStore()
const setCurrency = useSetCurrencyMutation()
const currencyError = ref<string | null>(null)

const selectedCurrency = computed(() =>
  isSupportedCurrency(auth.user?.currency) ? auth.user.currency : 'USD',
)

async function onCurrency(value: unknown) {
  if (!isSupportedCurrency(value) || value === selectedCurrency.value) return
  currencyError.value = null
  try {
    await setCurrency.mutateAsync({ currency: value })
  } catch (error) {
    currencyError.value =
      error instanceof ApiError ? error.message : 'Could not save the currency. Please try again.'
  }
}

// Accent — account-scoped like currency; the accent store repaints <html>'s
// data-accent as soon as the mutation replaces the auth user.
const accentStore = useAccentStore()
const setAccent = useSetAccentMutation()
const accentError = ref<string | null>(null)

async function onAccent(value: unknown) {
  if (!isAccent(value) || value === accentStore.accent) return
  accentError.value = null
  try {
    await setAccent.mutateAsync({ accent: value })
  } catch (error) {
    accentError.value =
      error instanceof ApiError ? error.message : 'Could not save the accent. Please try again.'
  }
}

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
      <p class="text-muted-foreground mt-2">Choose how TCGLense displays your collection.</p>
    </div>

    <div class="grid gap-6">
      <!-- Currency — unlike the device-local grid preferences, this follows the account. -->
      <Card>
        <CardHeader>
          <CardTitle class="flex items-center gap-2 text-lg">
            <CircleDollarSign class="size-5" /> Currency
          </CardTitle>
          <CardDescription>
            Show prices and collection values in your preferred currency on every device. Conversion
            uses daily reference rates; catalog values remain stored in USD.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Select
            :model-value="selectedCurrency"
            :disabled="setCurrency.isPending.value"
            @update:model-value="onCurrency"
          >
            <SelectTrigger class="w-full max-w-xs" aria-label="Display currency">
              <SelectValue placeholder="Choose a currency" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="option in CURRENCY_OPTIONS"
                :key="option.code"
                :value="option.code"
              >
                {{ option.code }} — {{ option.label }}
              </SelectItem>
            </SelectContent>
          </Select>
          <p v-if="currencyError" class="text-destructive mt-2 text-sm" role="alert">
            {{ currencyError }}
          </p>
        </CardContent>
      </Card>

      <!-- Accent color — the design system's brand hue; follows the account. -->
      <Card>
        <CardHeader>
          <CardTitle class="flex items-center gap-2 text-lg">
            <Palette class="size-5" /> Accent color
          </CardTitle>
          <CardDescription>
            The color TCGLense uses for buttons, links, and highlights, on every device you sign in
            on. Every preset keeps text readable in light and dark mode.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <!-- aria-pressed toggle buttons (the ChartSeriesLegend idiom), not radios — the
            radio role would promise arrow-key movement these separate buttons don't have. -->
          <div class="flex flex-wrap gap-2" role="group" aria-label="Accent color">
            <button
              v-for="option in ACCENT_OPTIONS"
              :key="option.value"
              type="button"
              :aria-pressed="accentStore.accent === option.value"
              :aria-label="option.label"
              :title="option.label"
              :disabled="setAccent.isPending.value"
              class="focus-visible:ring-ring/50 flex size-9 items-center justify-center rounded-full border transition-transform hover:scale-110 focus-visible:ring-3 disabled:pointer-events-none disabled:opacity-50 motion-reduce:transition-none"
              :class="accentStore.accent === option.value ? 'border-foreground/40' : ''"
              :style="{ backgroundColor: option.swatch }"
              @click="onAccent(option.value)"
            >
              <!-- The check is the non-color selection cue; its dark ink reads on
                every swatch (all are vivid mid-tones). -->
              <Check v-if="accentStore.accent === option.value" class="size-4 text-black/70" />
            </button>
          </div>
          <p v-if="accentError" class="text-destructive mt-2 text-sm" role="alert">
            {{ accentError }}
          </p>
        </CardContent>
      </Card>

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
            <Wallet class="size-5" /> Bulk threshold (USD)
          </CardTitle>
          <CardDescription>
            This cutoff is always measured in US dollars because catalog prices and filters remain
            canonical USD. Cards worth less than it count as “bulk” in your collection's value.
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
              currencyDisplay: 'code',
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
