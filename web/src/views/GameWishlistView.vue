<script setup lang="ts">
import { LayoutGrid } from '@lucide/vue'
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import { buttonVariants } from '@/components/ui/button'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import SetGridSkeleton from '@/components/cards/SetGridSkeleton.vue'
import SetGroupGrid from '@/components/cards/SetGroupGrid.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import CollectionSignInPrompt from '@/components/collection/CollectionSignInPrompt.vue'
import QuickAddBox from '@/components/collection/QuickAddBox.vue'
import SetsScopeToggle from '@/components/collection/SetsScopeToggle.vue'
import ProductHoldingSection from '@/components/products/ProductHoldingSection.vue'
import HoldingStatList from '@/components/shared/HoldingStatList.vue'
import { useGameName } from '@/composables/useCatalog'
import { useHoldingsLanding } from '@/composables/useHoldingsLanding'
import { useCurrency } from '@/composables/useCurrency'
import { sumUsd } from '@/lib/money'
import {
  useWishlistProductSummaryQuery,
  useWishlistSetsQuery,
  useWishlistSummaryQuery,
} from '@/composables/useWishlist'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'

// The per-game wish-list landing (issue #167). By default it lists just the sets
// holding wishlisted cards (like the collection landing lists only owned sets); a
// segmented toggle (`?sets=all`) flips it to the catalog game view's FULL set list —
// featured + year sections and all — so there's always somewhere to click through and
// start wishing. Either way the user's per-set wanted counts/values overlay the tiles
// that have any. The shared landing pipeline lives in `useHoldingsLanding` (the collection
// landing's twin); this view keeps the wish-list wording and the header's quick-add
// boxes — one for cards, one for sealed products (no import/sync — a wish list has
// nothing to sync from). Wanted sealed products use the shared ProductHoldingSection
// below the header. The card grids
// live on WishlistBrowseView (`/wishlist/:game/cards` + `.../sets/:code`).
const props = defineProps<{ game: string }>()
const money = useCurrency()

const {
  game,
  summary,
  heldSets: wishlistSets,
  showAllSets,
  setShowAllSets,
  catalogSets,
  sourceSets,
  filter,
  trimmedFilter,
  filtering,
  groups,
  relatedCount,
  activePending,
  activeError,
  sections,
  ownership,
  totalValue,
  hasStats,
} = useHoldingsLanding(props, {
  useSummaryQuery: useWishlistSummaryQuery,
  useHeldSetsQuery: useWishlistSetsQuery,
  basePath: '/wishlist',
  countNoun: 'wanted',
  withBulk: false,
})

// Sealed-product stats are fetched directly rather than through useHoldingsLanding, whose
// set-grouping and bulk-value concerns remain card-only. The trio self-hides
// (hasProductStats) until at least one product is wanted; while the query is pending it
// is simply absent, like the card trio's own hasStats gate. Every product write
// refreshes it for free — the query key sits in the ['wishlist-products', game] family
// that invalidateWishlistProducts prefix-invalidates.
const productSummaryQuery = useWishlistProductSummaryQuery(game)
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

// The cards section's own unique / total / value stats, under its heading below. No bulk
// slice (unlike the collection landing): a wish list is a shopping list, so only what it
// costs matters.
const cardStats = computed(() =>
  hasStats.value
    ? [
        { label: 'Unique cards', value: summary.value?.unique_cards.toLocaleString() ?? null },
        { label: 'Total copies', value: summary.value?.total_cards.toLocaleString() ?? null },
        { label: 'Total value', value: totalValue.value },
      ]
    : [],
)

const gameName = useGameName(game)
const auth = useAuthStore()

// Per-account page — kept out of search indexes.
usePageMeta({
  title: () => `Your ${gameName.value} wish list`,
  canonicalPath: () => `/wishlist/${game.value}`,
  noindex: true,
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <PageBreadcrumbs :items="[{ label: 'Wish list', to: '/wishlist' }, { label: gameName }]" />

    <!-- Signed out (session resolved): the wish-list routes are public, so rather than
         bouncing to the login page we prompt to sign in / sign up right here. While the
         initial session is still resolving, show the pending grid instead so a signed-in
         returning visitor never flashes the sign-in prompt. -->
    <CollectionSignInPrompt
      v-if="auth.sessionResolved && !auth.isAuthenticated"
      :game-name="gameName"
      list="wishlist"
    />

    <template v-else-if="auth.isAuthenticated">
      <header class="mb-6">
        <h1 class="text-3xl font-semibold tracking-tight">Your {{ gameName }} wish list</h1>
        <!-- The active mode's set count — just the wishlisted sets by default, the whole
             catalog under "All sets" — mirroring the catalog game view's header line. -->
        <p class="text-muted-foreground mt-1">
          {{ groups.length }} {{ groups.length === 1 ? 'set' : 'sets' }}
          <template v-if="relatedCount > 0"> · {{ relatedCount }} related</template>
          <template v-if="filtering"> matching “{{ trimmedFilter }}”</template>
        </p>

        <!-- Combined cards + sealed overview; the detailed per-section breakdowns live under
             the sealed and cards headings further down. -->
        <HoldingStatList :items="combinedStats" size="lg" class="mt-4" />

        <!-- Quick add: cards (name → printing → counts) and sealed products (name → quantity).
             Both write to the wish list. -->
        <div class="mt-5 grid max-w-3xl gap-4 sm:grid-cols-2">
          <div>
            <p class="text-muted-foreground mb-1.5 text-xs font-medium tracking-wide uppercase">
              Quick add a card
            </p>
            <QuickAddBox :game="game" list="wishlist" />
          </div>
          <div>
            <p class="text-muted-foreground mb-1.5 text-xs font-medium tracking-wide uppercase">
              Quick add a sealed product
            </p>
            <QuickAddBox :game="game" list="wishlist" kind="product" />
          </div>
        </div>
      </header>

      <!-- Wanted sealed products (issue #364): self-hides when nothing is wanted; each
           tile carries the hover quick-add control, and the sealed quick-add box above
           also writes here. -->
      <ProductHoldingSection :game="game" list="wishlist" class="mb-8" />

      <!-- Cards section heading + its own unique / total / value stats, matching the sealed
           section's heading + stats above. -->
      <h2 class="mb-4 text-lg font-semibold">Cards</h2>
      <HoldingStatList :items="cardStats" class="mb-6" />

      <!-- The set list — wishlisted sets by default, the whole catalog under "All sets".
           The filter bar sticks to the top of the viewport, and the all-mode year
           headings below offset against its fixed height (their sticky `top-15`),
           mirroring the catalog game view. -->
      <StickySearchBar class="mb-6 flex flex-wrap items-center gap-3">
        <!-- Which sets to list — the GroupViewToggle-style segmented control. -->
        <SetsScopeToggle
          :model-value="showAllSets"
          collected-label="Wishlisted"
          @update:model-value="setShowAllSets"
        />
        <CardSearchBox
          v-if="sourceSets.length"
          v-model="filter"
          class="w-full sm:w-64"
          aria-label="Filter sets by name or code"
          placeholder="Filter sets…"
        />
        <RouterLink
          :to="`/wishlist/${game}/cards`"
          :class="buttonVariants({ variant: 'default' })"
          class="shrink-0"
        >
          <LayoutGrid />
          View all cards
        </RouterLink>
      </StickySearchBar>

      <SetGridSkeleton v-if="activePending" />
      <p v-else-if="activeError" class="text-destructive py-12">
        Couldn't load sets. Please retry.
      </p>
      <p v-else-if="showAllSets && !catalogSets.length" class="text-muted-foreground py-12">
        No sets available yet.
      </p>
      <!-- Wishlisted mode with nothing wishlisted anywhere: offer the all-sets view,
           which is where adding starts. -->
      <div v-else-if="!showAllSets && !wishlistSets.length" class="py-16 text-center">
        <p class="text-muted-foreground">No sets with wishlisted cards yet.</p>
        <button
          type="button"
          :class="buttonVariants({ variant: 'default' })"
          class="mt-4 inline-flex"
          @click="setShowAllSets(true)"
        >
          <LayoutGrid />
          Show all sets to start adding
        </button>
      </div>
      <p v-else-if="filtering && !groups.length" class="text-muted-foreground py-12">
        No sets match “{{ trimmedFilter }}”.
      </p>

      <!-- All sets: the catalog game view's featured + year sections. -->
      <div v-else-if="showAllSets" class="space-y-10">
        <section v-for="section in sections" :key="section.key">
          <!-- Stuck below the sticky filter bar above (top-15 = its height) so the
               two stack rather than overlap at the top of the viewport. -->
          <div
            class="bg-background/85 sticky top-15 z-10 -mx-4 mb-3 flex items-baseline gap-2 border-b px-4 py-2 backdrop-blur"
          >
            <h2 class="text-xl font-semibold tracking-tight">{{ section.label }}</h2>
            <span class="text-muted-foreground text-sm">
              {{ section.groups.length }} {{ section.groups.length === 1 ? 'set' : 'sets' }}
            </span>
          </div>
          <SetGroupGrid
            :game="game"
            :groups="section.groups"
            :scroll-mt="28"
            base-path="/wishlist"
            :query="trimmedFilter"
            :ownership="ownership"
            count-noun="wanted"
          />
        </section>
      </div>

      <!-- Wishlisted sets only (the default): a flat newest-first grid, mirroring the
           collection landing's owned-sets view. -->
      <SetGroupGrid
        v-else
        :game="game"
        :groups="groups"
        :scroll-mt="28"
        base-path="/wishlist"
        :query="trimmedFilter"
        :ownership="ownership"
        count-noun="wanted"
      />
    </template>

    <!-- Initial session still resolving: reserve the set grid's layout. -->
    <SetGridSkeleton v-else />
  </div>
</template>
