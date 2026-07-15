<script setup lang="ts">
import { computed, defineAsyncComponent, h, ref } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { Loader2 } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { useCurrency } from '@/composables/useCurrency'
import { type PriceRange } from '@/lib/api'

// The shared price-history chart + range picker, used by both card and sealed-product
// detail pages. It's fed a `fetcher` (which takes the selected range and returns the USD
// series) and a base `queryKey`; the range is appended to that key so a range change
// refetches under a distinct cache entry. This wrapper owns the query, range state + the
// range buttons, and the pending/error/empty branches; the unovis chart body lives in
// PriceChartInner, loaded lazily so unovis stays off every detail route's critical chunk
// AND the query fires in parallel with that chunk fetch. The two USD fields are all the
// chart reads, so any series carrying them — a card's `PricePoint` (with unused eur/tix)
// or a product's `ProductPricePoint` — satisfies it.
interface PricePointLike {
  date: string
  usd: string | null
  usd_foil: string | null
}
const props = withDefaults(
  defineProps<{
    /** Base cache key (without the range); the selected range is appended. */
    queryKey: readonly unknown[]
    /** Fetch the USD price series for the given range. */
    fetcher: (range: PriceRange) => Promise<{ data: PricePointLike[] }>
    /** Card heading + range-group label. Defaults to the price-history wording; the
     * collection value chart overrides it. */
    title?: string
    /** Message shown when the selected range has no plottable data. */
    emptyText?: string
    /** Plot a single USD line with no foil series — the collection total. */
    singleSeries?: boolean
  }>(),
  {
    title: 'Price history',
    emptyText: 'No price history for this range.',
  },
)
const money = useCurrency()

// The chart body is a separate chunk so unovis never bloats a detail route; a Skeleton
// (reduced-motion aware, like every other placeholder) stands in immediately (delay 0)
// while that chunk loads.
const chartSkeleton = () => h(Skeleton, { class: 'h-64 w-full rounded-xl', 'aria-hidden': 'true' })
const PriceChartInner = defineAsyncComponent({
  loader: () => import('@/components/cards/PriceChartInner.vue'),
  loadingComponent: chartSkeleton,
  delay: 0,
})

// Selectable time window; longer ranges come back downsampled from the API. We default
// to 30 days — a daily (un-downsampled) window — so a young series shows every captured
// day. The 1y+ ranges bucket to weekly/coarser and keep only the last day per bucket,
// which collapses a handful of same-week days into a single point (misreads as "no
// history" on a fresh deployment).
const range = ref<PriceRange>('30d')
const RANGE_OPTIONS: { value: PriceRange; label: string }[] = [
  { value: '7d', label: '7D' },
  { value: '30d', label: '30D' },
  { value: '1y', label: '1Y' },
  { value: '2y', label: '2Y' },
  { value: '3y', label: '3Y' },
  { value: 'all', label: 'All' },
]
function selectRange(value: PriceRange) {
  range.value = value
}

// Public price-history endpoint, so a plain useQuery (no auth wrapper). Refs go straight
// into the queryKey so a card-to-card navigation (or a range change) refetches;
// keepPreviousData holds the current chart on screen while the next range loads instead
// of flashing the loading skeleton.
const query = useQuery({
  queryKey: computed(() => [...props.queryKey, range.value]),
  queryFn: () => props.fetcher(range.value),
  placeholderData: keepPreviousData,
})

const series = computed<PricePointLike[]>(() =>
  (query.data.value?.data ?? []).map((point) => ({
    ...point,
    usd: money.convertUsd(point.usd),
    usd_foil: money.convertUsd(point.usd_foil),
  })),
)

// "Empty" for a range means nothing plottable — either no rows at all, or (for the
// add-date-clamped collection series) every row null because the collection is younger than
// the window. Either way the chart body would draw a blank frame, so show emptyText instead.
const isEmpty = computed(
  () =>
    !query.isPending.value &&
    !query.isError.value &&
    !series.value.some((p) => p.usd != null || p.usd_foil != null),
)
</script>

<template>
  <Card class="mt-6">
    <CardHeader>
      <div class="flex flex-wrap items-center justify-between gap-2">
        <CardTitle class="text-sm font-semibold">{{ props.title }}</CardTitle>
        <div
          class="bg-muted/50 inline-flex items-center gap-1 rounded-lg p-0.5"
          role="group"
          :aria-label="`${props.title} range`"
        >
          <Button
            v-for="opt in RANGE_OPTIONS"
            :key="opt.value"
            type="button"
            :variant="range === opt.value ? 'secondary' : 'ghost'"
            size="sm"
            class="h-8 px-2.5 text-xs font-medium"
            :aria-pressed="range === opt.value"
            @click="selectRange(opt.value)"
          >
            <!-- In-flight cue on the just-picked range while its data loads (keepPreviousData
                 keeps the old chart up, so `isPlaceholderData` is the honest signal). -->
            <Loader2
              v-if="range === opt.value && query.isPlaceholderData.value"
              class="animate-spin"
            />
            {{ opt.label }}
          </Button>
        </div>
      </div>
    </CardHeader>
    <CardContent>
      <Skeleton v-if="query.isPending.value" class="h-64 w-full rounded-xl" aria-hidden="true" />
      <p v-else-if="query.isError.value" class="text-muted-foreground py-12 text-sm">
        Couldn't load price history.
      </p>
      <p v-else-if="isEmpty" class="text-muted-foreground py-12 text-sm">
        {{ props.emptyText }}
      </p>

      <PriceChartInner
        v-else
        :series="series"
        :range="range"
        :currency="money.displayCurrency.value"
        :single-series="props.singleSeries"
      />
    </CardContent>
  </Card>
</template>
