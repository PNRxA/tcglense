<script setup lang="ts">
import { computed, toRef } from 'vue'
import { RouterLink } from 'vue-router'
import { LayoutGrid } from '@lucide/vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import { buttonVariants } from '@/components/ui/button'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import SetGridSkeleton from '@/components/cards/SetGridSkeleton.vue'
import SetGroupGrid from '@/components/cards/SetGroupGrid.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import ProductHoldingSection from '@/components/products/ProductHoldingSection.vue'
import HoldingStatList from '@/components/shared/HoldingStatList.vue'
import { useGameName } from '@/composables/useCatalog'
import { useCurrency } from '@/composables/useCurrency'
import { useHoldingsLanding } from '@/composables/useHoldingsLanding'
import {
  usePublicWishlistProductSummaryQuery,
  usePublicWishlistSetsQuery,
  usePublicWishlistSummaryQuery,
} from '@/composables/usePublicWishlist'
import { sumUsd } from '@/lib/money'
import { usePageMeta } from '@/lib/seo'

// A user's public wish-list landing for a game (issue #493): the sets they want cards in
// (grouped + filterable, with per-set wanted counts/values) plus a "View all cards" link and
// the wanted sealed products. Read-only and indexable — it drives the *same* `useHoldingsLanding`
// engine as the authed wish-list landing, fed public (token-less) summary/sets queries. A 404
// (private/unknown handle or game) renders the not-found state. The wish-list twin of
// PublicCollectionView; a wish list is a shopping list, so no bulk slice.
const props = defineProps<{ handle: string; game: string }>()
const handle = toRef(props, 'handle')
const gameName = useGameName(toRef(props, 'game'))
const money = useCurrency()
// The owner's display handle is the username part of the URL handle (`alice-0001` → `alice`).
const username = computed(() => props.handle.replace(/-\d{1,4}$/, ''))

const {
  game,
  summary,
  groups,
  filter,
  trimmedFilter,
  filtering,
  sourceSets,
  activePending,
  ownership,
  totalValue,
  hasStats,
} = useHoldingsLanding(props, {
  useSummaryQuery: (g) => usePublicWishlistSummaryQuery(handle, g),
  useHeldSetsQuery: (g) => usePublicWishlistSetsQuery(handle, g),
  basePath: `/u/${props.handle}/wishlist`,
  countNoun: 'wanted',
  withBulk: false,
})

// The owner's public wanted sealed products. This query is also mounted by the sealed
// `ProductHoldingSection` below, so vue-query dedupes to one request.
const productSummaryQuery = usePublicWishlistProductSummaryQuery(handle, game)
const productSummary = computed(() => productSummaryQuery.data.value)
const hasProductStats = computed(() => (productSummary.value?.unique_products ?? 0) > 0)

// Top-of-page combined overview (cards + sealed rolled together), the headline above the
// per-section breakdowns; empty (so it self-hides) until at least one holding is wanted.
const combinedStats = computed(() => {
  if (!hasStats.value && !hasProductStats.value) return []
  const cards = summary.value
  const products = productSummary.value
  return [
    {
      label: 'Unique items',
      value: ((cards?.unique_cards ?? 0) + (products?.unique_products ?? 0)).toLocaleString(),
    },
    {
      label: 'Total items',
      value: ((cards?.total_cards ?? 0) + (products?.total_products ?? 0)).toLocaleString(),
    },
    {
      label: 'Total value',
      value: money.formatUsd(sumUsd(cards?.total_value_usd, products?.total_value_usd)),
    },
  ]
})

// The cards section's own unique / total / value stats, under its heading below. No bulk slice
// (unlike the collection landing): a wish list is a shopping list, so only what it costs matters.
const cardStats = computed(() =>
  hasStats.value
    ? [
        { label: 'Unique cards', value: summary.value?.unique_cards.toLocaleString() ?? null },
        { label: 'Total copies', value: summary.value?.total_cards.toLocaleString() ?? null },
        { label: 'Total value', value: totalValue.value },
      ]
    : [],
)

// The landing's `activeError` flips with the inherited `?sets=all` scope toggle (it then reads
// the always-200 public catalog list), so gate not-found on the handle-scoped summary query
// instead — it 404s for a private/unknown handle regardless of the toggle. Same key as the
// engine's own summary read, so this dedupes to a single request.
const summaryQuery = usePublicWishlistSummaryQuery(handle, game)
const notFound = computed(() => summaryQuery.isError.value)

usePageMeta({
  title: () => `${username.value}'s ${gameName.value} wish list`,
  description: () => `${username.value}'s public ${gameName.value} wish list on TCGLense.`,
  canonicalPath: () => `/u/${handle.value}/wishlist/${game.value}`,
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <div v-if="notFound" class="py-20 text-center">
      <h1 class="text-2xl font-semibold tracking-tight">Wish list not found</h1>
      <p class="text-muted-foreground mt-2">This wish list is private or doesn't exist.</p>
      <RouterLink to="/" class="text-primary mt-4 inline-block underline underline-offset-2">
        Go home
      </RouterLink>
    </div>

    <template v-else>
      <PageBreadcrumbs
        :items="[{ label: `@${username}`, to: `/u/${handle}` }, { label: `${gameName} wish list` }]"
      />

      <header class="mb-6">
        <h1 class="text-3xl font-semibold tracking-tight">
          {{ username }}'s {{ gameName }} wish list
        </h1>
        <!-- Combined cards + sealed overview; the detailed per-section breakdowns live under
             the sealed and cards headings below (matching the authed wish-list landing). -->
        <HoldingStatList :items="combinedStats" size="lg" class="mt-4" />
      </header>

      <!-- The owner's wanted sealed products (read-only public mirror); renders nothing when the
           owner wants none, so a card-only wish list looks exactly as before. -->
      <ProductHoldingSection :game="game" :handle="handle" list="wishlist" class="mb-8" />

      <!-- Cards section heading + its own unique / total / value stats, matching the sealed
           section's heading + stats above. -->
      <h2 class="mb-4 text-lg font-semibold">Cards</h2>
      <HoldingStatList :items="cardStats" class="mb-6" />

      <StickySearchBar class="mb-6 flex flex-wrap items-center gap-3">
        <CardSearchBox
          v-if="sourceSets.length"
          v-model="filter"
          class="w-full sm:w-64"
          aria-label="Filter sets by name or code"
          placeholder="Filter sets…"
        />
        <RouterLink
          :to="`/u/${handle}/wishlist/${game}/cards`"
          :class="buttonVariants({ variant: 'default' })"
          class="shrink-0"
        >
          <LayoutGrid />
          View all cards
        </RouterLink>
      </StickySearchBar>

      <SetGridSkeleton v-if="activePending" />
      <p v-else-if="!sourceSets.length" class="text-muted-foreground py-12">
        No cards on this wish list.
      </p>
      <p v-else-if="filtering && !groups.length" class="text-muted-foreground py-12">
        No sets match “{{ trimmedFilter }}”.
      </p>
      <!-- Wanted sets (grouped, with nested sub-sets), each tile carrying the owner's wanted
           counts and linking to that set's public wish-list card view. -->
      <SetGroupGrid
        v-else
        :game="game"
        :groups="groups"
        :scroll-mt="28"
        :base-path="`/u/${handle}/wishlist`"
        :query="trimmedFilter"
        :ownership="ownership"
        count-noun="wanted"
      />
    </template>
  </div>
</template>
