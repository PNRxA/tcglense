<script setup lang="ts">
import { computed, ref } from 'vue'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import BreakdownBars from '@/components/holdings/BreakdownBars.vue'
import TopHoldingRow from '@/components/holdings/TopHoldingRow.vue'
import CollapsibleSection from '@/components/shared/CollapsibleSection.vue'
import type { HoldingBreakdown } from '@/lib/api'
import { BREAKDOWN_FACETS, hasBreakdown, type BreakdownFacet } from '@/lib/holdingBreakdown'
import type { CountNoun } from '@/lib/ownership'

// The holdings landing's breakdown panel (issue #680): "where is my money?". A facet switch
// (rarity / colour / type / finish) over a bar list on the left and the top holdings by held
// value on the right — the same panel for the collection and the wish list, which differ
// only in the count noun (owned / wanted). Everything it draws is the server's
// `GET …/breakdown` response, read through `useHoldingsLanding`; the panel itself is
// presentational so the two landings can't compute it differently.
//
// It **rests collapsed** behind the shared `CollapsibleSection` disclosure, like the movers
// and value-chart panels above it: the body mounts only while open, and the landing engine
// gates the (whole-holdings) query on the bound `expanded` state, so a collapsed panel
// fetches nothing. Per-mount state, deliberately not remembered (see `DeckStats`).
const props = defineProps<{
  game: string
  breakdown: HoldingBreakdown | undefined
  pending: boolean
  error: boolean
  /** The copy noun — `owned` (collection) or `wanted` (wish list); the panel's surface
   * wording (the title, the blurb, the top list's heading) derives from it. */
  countNoun: CountNoun
}>()

/** Whether the body is open — bound by the landing so it can gate the query on it. */
const expanded = defineModel<boolean>('expanded', { default: false })

const facet = ref<BreakdownFacet>('rarity')
const buckets = computed(() => props.breakdown?.[facet.value] ?? [])
const top = computed(() => props.breakdown?.top ?? [])
const unpriced = computed(() => props.breakdown?.unpriced_cards ?? 0)
const empty = computed(() => !hasBreakdown(props.breakdown))

// A wish list is a shopping list, so its wording is cost, not holdings; every string comes
// off the one noun so the two landings can't pass a mismatched set.
const wanted = computed(() => props.countNoun === 'wanted')
const title = computed(() => (wanted.value ? 'Where the cost is' : 'Where the value is'))
const blurb = computed(() =>
  wanted.value
    ? 'Cost and copies by rarity, colour, type and finish, and your most expensive wanted cards.'
    : 'Value and copies by rarity, colour, type and finish, and your most valuable holdings.',
)
const topTitle = computed(() =>
  wanted.value ? 'Most expensive wanted cards' : 'Top holdings by value',
)
</script>

<template>
  <CollapsibleSection v-model:expanded="expanded" :title="title" :blurb="blurb" heading="h3">
    <div class="mb-4 flex flex-wrap items-center justify-end gap-2">
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
    <p v-else-if="empty" class="text-muted-foreground py-10 text-center text-sm">
      Nothing to break down yet.
    </p>
    <div v-else class="grid gap-x-8 gap-y-6 sm:grid-cols-2">
      <!-- `min-w-0`: grid items refuse to shrink below their content's min width, which
           let a long card name push the value column off the card on a narrow column. -->
      <section class="min-w-0">
        <BreakdownBars :facet="facet" :buckets="buckets" :count-noun="countNoun" />
        <p v-if="unpriced > 0" class="text-muted-foreground mt-3 text-xs">
          {{ unpriced.toLocaleString() }} {{ unpriced === 1 ? 'card has' : 'cards have' }} no price
          and {{ unpriced === 1 ? "isn't" : "aren't" }} counted in any value.
        </p>
      </section>
      <section class="min-w-0">
        <h4 class="text-muted-foreground text-xs font-semibold tracking-wide uppercase">
          {{ topTitle }}
        </h4>
        <ul v-if="top.length" class="mt-2 space-y-1">
          <li v-for="holding in top" :key="holding.card.id">
            <TopHoldingRow :game="game" :holding="holding" :count-noun="countNoun" />
          </li>
        </ul>
        <p v-else class="text-muted-foreground mt-2 text-sm">Nothing priced yet.</p>
      </section>
    </div>
  </CollapsibleSection>
</template>
