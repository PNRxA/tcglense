<script setup lang="ts">
import { computed, ref } from 'vue'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import BreakdownBars from '@/components/holdings/BreakdownBars.vue'
import TopHoldingRow from '@/components/holdings/TopHoldingRow.vue'
import type { HoldingBreakdown } from '@/lib/api'
import { BREAKDOWN_FACETS, hasBreakdown, type BreakdownFacet } from '@/lib/holdingBreakdown'
import type { CountNoun } from '@/lib/ownership'

// The holdings landing's breakdown panel (issue #680): "where is my money?". One card with
// a facet switch (rarity / colour / type / finish) over a bar list on the left and the top
// holdings by held value on the right — the same panel for the collection and the wish
// list, which differ only in the count noun (owned / wanted). Everything it
// draws is the server's `GET …/breakdown` response, read through `useHoldingsLanding`;
// the panel itself is presentational so the two landings can't compute it differently.
//
// The card renders nothing when there is nothing to draw (an empty holding answers empty
// facets everywhere) — the pending/error states still show, so the panel doesn't pop in
// after the rest of the page has settled.
const props = defineProps<{
  game: string
  breakdown: HoldingBreakdown | undefined
  pending: boolean
  error: boolean
  /** The copy noun — `owned` (collection) or `wanted` (wish list); the panel's surface
   * wording (the title, the top list's heading) derives from it. */
  countNoun: CountNoun
}>()

const facet = ref<BreakdownFacet>('rarity')
const buckets = computed(() => props.breakdown?.[facet.value] ?? [])
const top = computed(() => props.breakdown?.top ?? [])
const unpriced = computed(() => props.breakdown?.unpriced_cards ?? 0)
const visible = computed(() => props.pending || props.error || hasBreakdown(props.breakdown))
// A wish list is a shopping list, so its wording is cost, not holdings; both strings come
// off the one noun so the two landings can't pass a mismatched pair.
const title = computed(() =>
  props.countNoun === 'wanted' ? 'Where the cost is' : 'Where the value is',
)
const topTitle = computed(() =>
  props.countNoun === 'wanted' ? 'Most expensive wanted cards' : 'Top holdings by value',
)
</script>

<template>
  <Card v-if="visible" class="mt-6">
    <CardHeader>
      <div class="flex flex-wrap items-center justify-between gap-2">
        <CardTitle class="text-sm font-semibold">{{ title }}</CardTitle>
        <div
          class="bg-muted/50 flex max-w-full flex-wrap items-center justify-end gap-1 rounded-lg p-0.5"
          role="group"
          aria-label="Breakdown facet"
        >
          <Button
            v-for="opt in BREAKDOWN_FACETS"
            :key="opt.key"
            type="button"
            :variant="facet === opt.key ? 'secondary' : 'ghost'"
            size="sm"
            class="h-8 px-2.5 text-xs font-medium"
            :aria-pressed="facet === opt.key"
            @click="facet = opt.key"
          >
            {{ opt.label }}
          </Button>
        </div>
      </div>
    </CardHeader>
    <CardContent>
      <div v-if="pending" class="grid gap-x-8 gap-y-4 sm:grid-cols-2" aria-hidden="true">
        <div class="space-y-3">
          <Skeleton v-for="row in 4" :key="row" class="h-6 w-full" />
        </div>
        <div class="space-y-2">
          <Skeleton class="h-4 w-24" />
          <Skeleton v-for="row in 3" :key="row" class="h-12 w-full" />
        </div>
      </div>
      <p v-else-if="error" class="text-muted-foreground py-12 text-sm">
        Couldn't load the breakdown.
      </p>
      <div v-else class="grid gap-x-8 gap-y-6 sm:grid-cols-2">
        <!-- `min-w-0`: grid items refuse to shrink below their content's min width, which
             let a long card name push the value column off the card on a narrow column. -->
        <section class="min-w-0">
          <BreakdownBars :facet="facet" :buckets="buckets" :count-noun="countNoun" />
          <p v-if="unpriced > 0" class="text-muted-foreground mt-3 text-xs">
            {{ unpriced.toLocaleString() }} {{ unpriced === 1 ? 'card has' : 'cards have' }} no
            price and {{ unpriced === 1 ? "isn't" : "aren't" }} counted in any value.
          </p>
        </section>
        <section class="min-w-0">
          <h3 class="text-muted-foreground text-xs font-semibold tracking-wide uppercase">
            {{ topTitle }}
          </h3>
          <ul v-if="top.length" class="mt-2 space-y-1">
            <li v-for="holding in top" :key="holding.card.id">
              <TopHoldingRow :game="game" :holding="holding" :count-noun="countNoun" />
            </li>
          </ul>
          <p v-else class="text-muted-foreground mt-2 text-sm">Nothing priced yet.</p>
        </section>
      </div>
    </CardContent>
  </Card>
</template>
