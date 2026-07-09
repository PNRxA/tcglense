<script setup lang="ts">
import { computed, toRef } from 'vue'
import { LayoutGrid, ScanLine } from '@lucide/vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import { buttonVariants } from '@/components/ui/button'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import PriceChart from '@/components/cards/PriceChart.vue'
import SetGridSkeleton from '@/components/cards/SetGridSkeleton.vue'
import SetGroupGrid from '@/components/cards/SetGroupGrid.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import CollectionSignInPrompt from '@/components/collection/CollectionSignInPrompt.vue'
import CollectionSyncControls from '@/components/collection/CollectionSyncControls.vue'
import QuickAddBox from '@/components/collection/QuickAddBox.vue'
import { useGameName, useSetsQuery } from '@/composables/useCatalog'
import { useFilteredSetGroups } from '@/composables/useSetGrouping'
import { useCollectionSetsQuery, useCollectionSummaryQuery } from '@/composables/useCollection'
import { getCollectionValueHistory } from '@/lib/api'
import { formatUsd } from '@/lib/money'
import { usePageMeta } from '@/lib/seo'
import { groupByYear, partitionPinned } from '@/lib/setGroups'
import { cn } from '@/lib/utils'
import { useAuthStore } from '@/stores/auth'
import type { CardSet, PriceRange } from '@/lib/api'

// The per-game collection landing: pick a set to see just your cards from it, or "All
// cards" for the whole collection. By default it lists just the sets you own cards in;
// a segmented toggle (`?sets=all`) flips it to the catalog game view's FULL set list —
// featured + year sections and all — the same Collected/All-sets control as the
// wish-list landing. Either way the per-set owned counts/values overlay the tiles that
// have any. The header carries the value/count summary plus the import / re-sync
// controls; the actual card grids live on CollectionBrowseView
// (`/collection/:game/cards` + `.../sets/:code`).
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')
const gameName = useGameName(game)

const auth = useAuthStore()

// Per-account page — kept out of search indexes.
usePageMeta({
  title: () => `Your ${gameName.value} collection`,
  canonicalPath: () => `/collection/${game.value}`,
  noindex: true,
})

const summaryQuery = useCollectionSummaryQuery(game)
// The sets holding owned cards — the default mode's list and the per-set overlay
// either mode shows.
const collectionSetsQuery = useCollectionSetsQuery(game)

const summary = computed(() => summaryQuery.data.value)
const ownedSets = computed(() => collectionSetsQuery.data.value?.data ?? [])

// Which sets the grid lists: just the owned ones (default) or the whole catalog
// (`?sets=all`). A URL param, like the browse views' `?ghosts`, so the choice survives
// navigation and the back button.
const route = useRoute()
const router = useRouter()
const showAllSets = computed(() => route.query.sets === 'all')
function setShowAllSets(on: boolean) {
  const next = { ...route.query }
  if (on) next.sets = 'all'
  else delete next.sets
  router.replace({ query: next })
}

// The FULL public set list (shared, cached with the catalog game view) — the all-sets
// mode's source, fetched unconditionally so toggling never starts from a spinner.
const catalogSetsQuery = useSetsQuery(game)
const catalogSets = computed(() => catalogSetsQuery.data.value?.data ?? [])

// The active mode's sets, grouped and filterable exactly like the catalog game view:
// nested sub-sets, instant name/code narrowing, groups kept whole when any member
// matches (issues #127/#128). One grouping instance over the switched source, so the
// filter box and header counts track whichever mode is on.
const sourceSets = computed<CardSet[]>(() =>
  showAllSets.value ? catalogSets.value : ownedSets.value,
)
const { filter, trimmedFilter, filtering, groups, relatedCount } = useFilteredSetGroups(
  game,
  sourceSets,
)

// The active mode's query state, for the loading/error rows below.
const activePending = computed(() =>
  showAllSets.value ? catalogSetsQuery.isPending.value : collectionSetsQuery.isPending.value,
)
const activeError = computed(() =>
  showAllSets.value ? catalogSetsQuery.isError.value : collectionSetsQuery.isError.value,
)

// Pinned sets (e.g. Secret Lair) lead as a "Featured" section; the rest break into
// release-year sections — the same scannable layout as the catalog game view. Used by
// the all-sets mode only (the owned-sets default is a flat newest-first grid).
const partitioned = computed(() => partitionPinned(groups.value))
const years = computed(() => groupByYear(partitioned.value.rest))
const yearLabel = (year: number | null) => (year === null ? 'Unknown year' : String(year))
const sections = computed(() => {
  const featured = partitioned.value.pinned
  const yearSections = years.value.map((section) => ({
    key: section.year === null ? 'unknown' : String(section.year),
    label: yearLabel(section.year),
    groups: section.groups,
  }))
  return featured.length
    ? [{ key: 'featured', label: 'Featured', groups: featured }, ...yearSections]
    : yearSections
})

// Per-set-code owned stats each tile shows next to its name: the "N/M owned" completion
// count, the "N copies" total (when you own duplicates, issue #125), and the preformatted
// owned value (issue #119; null/unpriced sets carry a null the tile omits) plus its bulk
// slice. Built in one pass and passed to SetGroupGrid as a single `ownership` object;
// sets you own nothing in are simply absent, so in all-sets mode their tiles keep the
// plain catalog card count.
const ownership = computed(() => {
  const counts: Record<string, number> = {}
  const copies: Record<string, number> = {}
  const values: Record<string, string | null> = {}
  const bulkValues: Record<string, string | null> = {}
  for (const set of ownedSets.value) {
    counts[set.code] = set.owned_cards
    copies[set.code] = set.owned_copies
    values[set.code] = formatUsd(set.owned_value_usd)
    bulkValues[set.code] = formatUsd(set.owned_bulk_value_usd)
  }
  return { counts, copies, values, bulkValues }
})
const totalValue = computed(() => formatUsd(summary.value?.total_value_usd))
// The bulk (< $1/card) slice of the total value (issue: bulk-card value). Present
// whenever the total is (both gate on something being priced).
const bulkValue = computed(() => formatUsd(summary.value?.bulk_value_usd))

// Stats are worth showing only once something is owned.
const hasStats = computed(() => (summary.value?.unique_cards ?? 0) > 0)

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
        <div class="bg-muted text-muted-foreground inline-flex shrink-0 rounded-md p-0.5 text-sm">
          <button
            type="button"
            :class="
              cn(
                'rounded px-3 py-1.5 font-medium transition-colors',
                !showAllSets ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
              )
            "
            @click="setShowAllSets(false)"
          >
            Collected
          </button>
          <button
            type="button"
            :class="
              cn(
                'rounded px-3 py-1.5 font-medium transition-colors',
                showAllSets ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
              )
            "
            @click="setShowAllSets(true)"
          >
            All sets
          </button>
        </div>
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
