<script setup lang="ts">
import { LayoutGrid, ScanLine } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import { buttonVariants } from '@/components/ui/button'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import PriceChart from '@/components/cards/PriceChart.vue'
import SetGridSkeleton from '@/components/cards/SetGridSkeleton.vue'
import SetGroupGrid from '@/components/cards/SetGroupGrid.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import CollectionSignInPrompt from '@/components/collection/CollectionSignInPrompt.vue'
import CollectionSyncControls from '@/components/collection/CollectionSyncControls.vue'
import CollectionVisibilityCard from '@/components/collection/CollectionVisibilityCard.vue'
import QuickAddBox from '@/components/collection/QuickAddBox.vue'
import SetsScopeToggle from '@/components/collection/SetsScopeToggle.vue'
import { useGameName } from '@/composables/useCatalog'
import { useCollectionSetsQuery, useCollectionSummaryQuery } from '@/composables/useCollection'
import { useHoldingsLanding } from '@/composables/useHoldingsLanding'
import { getCollectionValueHistory } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'
import type { PriceRange } from '@/lib/api'

// The per-game collection landing: pick a set to see just your cards from it, or "All
// cards" for the whole collection. By default it lists just the sets you own cards in;
// a segmented toggle (`?sets=all`) flips it to the catalog game view's FULL set list —
// featured + year sections and all — the same Collected/All-sets control as the
// wish-list landing. Either way the per-set owned counts/values overlay the tiles that
// have any. The shared landing pipeline (scope toggle, filter + grouping, sectioning,
// ownership map, header stats) lives in `useHoldingsLanding`; this view layers on the
// collection-only extras (sync controls, camera scan, value-history chart, bulk-value
// stat). The actual card grids live on CollectionBrowseView (`/collection/:game/cards` +
// `.../sets/:code`).
const props = defineProps<{ game: string }>()

const {
  game,
  summary,
  heldSets: ownedSets,
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
  bulkValue,
  hasStats,
} = useHoldingsLanding(props, {
  useSummaryQuery: useCollectionSummaryQuery,
  useHeldSetsQuery: useCollectionSetsQuery,
  basePath: '/collection',
  withBulk: true,
})

const gameName = useGameName(game)
const auth = useAuthStore()

// Per-account page — kept out of search indexes.
usePageMeta({
  title: () => `Your ${gameName.value} collection`,
  canonicalPath: () => `/collection/${game.value}`,
  noindex: true,
})

// Value-over-time chart fetcher. PriceChart owns its own query (it appends the selected
// range to the base key); the read goes through the auth store so an expired access token
// refreshes transparently — the endpoint is per-user and authenticated.
function fetchValueHistory(range: PriceRange) {
  return auth.authFetch((token) => getCollectionValueHistory(token, game.value, range))
}
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <PageBreadcrumbs :items="[{ label: 'Collection', to: '/collection' }, { label: gameName }]" />

    <!-- Signed out (session resolved): the collection routes are public, so rather than
         bouncing to the login page we prompt to sign in / sign up right here. While the
         initial session is still resolving, show the pending grid instead so a signed-in
         returning visitor never flashes the sign-in prompt. -->
    <CollectionSignInPrompt
      v-if="auth.sessionResolved && !auth.isAuthenticated"
      :game-name="gameName"
    />

    <template v-else-if="auth.isAuthenticated">
      <header class="mb-6">
        <h1 class="text-3xl font-semibold tracking-tight">Your {{ gameName }} collection</h1>
        <!-- The active mode's set count — just the owned sets by default, the whole
             catalog under "All sets" — mirroring the catalog game view's header line. -->
        <p class="text-muted-foreground mt-1">
          {{ groups.length }} {{ groups.length === 1 ? 'set' : 'sets' }}
          <template v-if="relatedCount > 0"> · {{ relatedCount }} related</template>
          <template v-if="filtering"> matching “{{ trimmedFilter }}”</template>
        </p>

        <!-- Summary stats: distinct cards, total copies, estimated value. -->
        <dl v-if="hasStats" class="mt-4 flex flex-wrap gap-x-8 gap-y-3">
          <div>
            <dt class="text-muted-foreground text-xs tracking-wide uppercase">Unique cards</dt>
            <dd class="text-xl font-semibold tabular-nums">
              {{ summary?.unique_cards.toLocaleString() }}
            </dd>
          </div>
          <div>
            <dt class="text-muted-foreground text-xs tracking-wide uppercase">Total copies</dt>
            <dd class="text-xl font-semibold tabular-nums">
              {{ summary?.total_cards.toLocaleString() }}
            </dd>
          </div>
          <div v-if="totalValue">
            <dt class="text-muted-foreground text-xs tracking-wide uppercase">Total value</dt>
            <dd class="text-xl font-semibold tabular-nums">{{ totalValue }}</dd>
          </div>
          <!-- The bulk (< $1/card) slice of the total, so it's clear how much of the
               collection's value is chaff vs. real money. -->
          <div v-if="bulkValue">
            <dt class="text-muted-foreground text-xs tracking-wide uppercase">Bulk value</dt>
            <dd class="text-xl font-semibold tabular-nums">{{ bulkValue }}</dd>
          </div>
        </dl>

        <!-- Quick add: type a name, pick a printing, add regular/foil — without leaving
             this page. Useful both to seed an empty collection and to top up an
             existing one, so it's shown regardless of what's owned. -->
        <div class="mt-5 max-w-md">
          <p class="text-muted-foreground mb-1.5 text-xs font-medium tracking-wide uppercase">
            Quick add a card
          </p>
          <QuickAddBox :game="game" />
          <!-- Or bulk-add with the camera: OCR a physical card and drop it straight in. -->
          <RouterLink
            to="/scan"
            :class="buttonVariants({ variant: 'outline', size: 'sm' })"
            class="mt-2 inline-flex"
          >
            <ScanLine />
            Scan cards with your camera
          </RouterLink>
        </div>

        <CollectionSyncControls :game="game" />

        <!-- Make this game's collection public and get a shareable link (issues #361/#362).
             Per-game: sharing MTG doesn't share any other game's collection. -->
        <CollectionVisibilityCard :game="game" />
      </header>

      <!-- Total collection value over time — reconstructed from historic prices and each
           card's add-date. Shown once something is owned; reuses the shared price-history
           chart to render the single total-value line. -->
      <PriceChart
        v-if="hasStats"
        title="Collection value"
        empty-text="No value history for this range yet."
        single-series
        :query-key="['collection-value-history', game]"
        :fetcher="fetchValueHistory"
      />

      <!-- The set list — owned sets by default, the whole catalog under "All sets".
           The filter bar sticks to the top of the viewport, and the all-mode year
           headings below offset against its fixed height (their sticky `top-15`),
           mirroring the catalog game view. -->
      <StickySearchBar class="mb-6 flex flex-wrap items-center gap-3">
        <!-- Which sets to list — the GroupViewToggle-style segmented control. -->
        <SetsScopeToggle
          :model-value="showAllSets"
          collected-label="Collected"
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
          :to="`/collection/${game}/cards`"
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
      <!-- Collected mode with nothing owned anywhere: offer the all-sets view, which is
           where adding starts. -->
      <div v-else-if="!showAllSets && !ownedSets.length" class="py-16 text-center">
        <p class="text-muted-foreground">Your {{ gameName }} collection is empty.</p>
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
            base-path="/collection"
            :query="trimmedFilter"
            :ownership="ownership"
          />
        </section>
      </div>

      <!-- Owned sets only (the default): a flat newest-first grid. Owned sub-sets nest
           under their parent (SetGroup), matching the catalog game view; a childless
           owned set stays a plain tile. Both link to the collection's per-set view and
           show owned counts. -->
      <SetGroupGrid
        v-else
        :game="game"
        :groups="groups"
        :scroll-mt="28"
        base-path="/collection"
        :query="trimmedFilter"
        :ownership="ownership"
      />
    </template>

    <!-- Initial session still resolving: reserve the set grid's layout. -->
    <SetGridSkeleton v-else />
  </div>
</template>
