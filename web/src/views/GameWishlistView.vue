<script setup lang="ts">
import { computed, toRef } from 'vue'
import { LayoutGrid } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import { buttonVariants } from '@/components/ui/button'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import SetGroupGrid from '@/components/cards/SetGroupGrid.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import CollectionSignInPrompt from '@/components/collection/CollectionSignInPrompt.vue'
import QuickAddBox from '@/components/collection/QuickAddBox.vue'
import { useGameName, useSetsQuery } from '@/composables/useCatalog'
import { useFilteredSetGroups } from '@/composables/useSetGrouping'
import { useWishlistSetsQuery, useWishlistSummaryQuery } from '@/composables/useWishlist'
import { formatUsd } from '@/lib/money'
import { usePageMeta } from '@/lib/seo'
import { groupByYear, partitionPinned } from '@/lib/setGroups'
import { useAuthStore } from '@/stores/auth'

// The per-game wish-list landing (issue #167). Unlike the collection landing (which
// lists only owned sets), this mirrors the catalog's game view: EVERY set, featured +
// year sections and all, so there's always somewhere to click through and start
// wishing — with the user's per-set wanted counts/values overlaid on the tiles that
// have any. The header carries the value/count summary and a quick-add box (no
// import/sync — a wish list has nothing to sync from); the card grids live on
// WishlistBrowseView (`/wishlist/:game/cards` + `.../sets/:code`).
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')
const gameName = useGameName(game)

const auth = useAuthStore()

// Per-account page — kept out of search indexes.
usePageMeta({
  title: () => `Your ${gameName.value} wish list`,
  canonicalPath: () => `/wishlist/${game.value}`,
  noindex: true,
})

const summaryQuery = useWishlistSummaryQuery(game)
// The sets holding wishlisted cards — only the per-set counts/values overlay, since the
// grid itself lists the whole catalog.
const wishlistSetsQuery = useWishlistSetsQuery(game)

const summary = computed(() => summaryQuery.data.value)
const wishlistSets = computed(() => wishlistSetsQuery.data.value?.data ?? [])

// The FULL public set list (shared, cached with the catalog game view), grouped and
// filterable exactly like it: nested sub-sets, instant name/code narrowing, groups kept
// whole when any member matches (issues #127/#128).
const setsQuery = useSetsQuery(game)
const sets = computed(() => setsQuery.data.value?.data ?? [])
const { filter, trimmedFilter, filtering, groups, relatedCount } = useFilteredSetGroups(game, sets)

// Pinned sets (e.g. Secret Lair) lead as a "Featured" section; the rest break into
// release-year sections — the same scannable layout as the catalog game view.
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

// Per-set-code wanted stats each tile shows next to its name: the "N/M wanted"
// completion count, the "N copies" total (duplicates), and the preformatted value
// its wanted cards would cost (null/unpriced sets carry a null the tile omits) — the
// wish-list mirror of the collection landing's ownership object, built in one pass
// and passed to SetGroupGrid. Sets with nothing wishlisted are simply absent, so
// their tiles keep the plain catalog card count.
const ownership = computed(() => {
  const counts: Record<string, number> = {}
  const copies: Record<string, number> = {}
  const values: Record<string, string | null> = {}
  const bulkValues: Record<string, string | null> = {}
  for (const set of wishlistSets.value) {
    counts[set.code] = set.owned_cards
    copies[set.code] = set.owned_copies
    values[set.code] = formatUsd(set.owned_value_usd)
    bulkValues[set.code] = formatUsd(set.owned_bulk_value_usd)
  }
  return { counts, copies, values, bulkValues }
})
const totalValue = computed(() => formatUsd(summary.value?.total_value_usd))
// The bulk (< $1/card) slice of the total value, mirroring the collection landing.
const bulkValue = computed(() => formatUsd(summary.value?.bulk_value_usd))

// Stats are worth showing only once something is wanted.
const hasStats = computed(() => (summary.value?.unique_cards ?? 0) > 0)
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <PageBreadcrumbs :items="[{ label: 'Wish list', to: '/wishlist' }, { label: gameName }]" />

    <!-- Signed out: the wish-list routes are public, so rather than bouncing to the
         login page we prompt to sign in / sign up right here. -->
    <CollectionSignInPrompt v-if="!auth.isAuthenticated" :game-name="gameName" list="wishlist" />

    <template v-else>
      <header class="mb-6">
        <h1 class="text-3xl font-semibold tracking-tight">Your {{ gameName }} wish list</h1>
        <!-- The whole catalog's set count (this landing lists every set, wishlisted or
             not), mirroring the catalog game view's header line. -->
        <p class="text-muted-foreground mt-1">
          {{ groups.length }} {{ groups.length === 1 ? 'set' : 'sets' }}
          <template v-if="relatedCount > 0"> · {{ relatedCount }} related</template>
          <template v-if="filtering"> matching “{{ trimmedFilter }}”</template>
        </p>

        <!-- Summary stats: distinct cards, total copies, what they'd cost. -->
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
               wish list's cost is chaff vs. real money. -->
          <div v-if="bulkValue">
            <dt class="text-muted-foreground text-xs tracking-wide uppercase">Bulk value</dt>
            <dd class="text-xl font-semibold tabular-nums">{{ bulkValue }}</dd>
          </div>
        </dl>

        <!-- Quick add: type a name, pick a printing, add regular/foil — without leaving
             this page. Useful both to seed an empty wish list and to grow one, so it's
             shown regardless of what's wanted. -->
        <div class="mt-5 max-w-md">
          <p class="text-muted-foreground mb-1.5 text-xs font-medium tracking-wide uppercase">
            Quick add a card
          </p>
          <QuickAddBox :game="game" list="wishlist" />
        </div>
      </header>

      <!-- Every set in the catalog (not just the wishlisted ones) — the filter bar sticks
           to the top of the viewport, and the year headings below offset against its fixed
           height (their sticky `top-15`), mirroring the catalog game view. -->
      <StickySearchBar class="mb-6 flex items-center gap-3">
        <CardSearchBox
          v-if="sets.length"
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

      <LoadingRow v-if="setsQuery.isPending.value" label="Loading sets…" />
      <p v-else-if="setsQuery.isError.value" class="text-destructive py-12">
        Couldn't load sets. Please retry.
      </p>
      <p v-else-if="!sets.length" class="text-muted-foreground py-12">No sets available yet.</p>
      <p v-else-if="filtering && !groups.length" class="text-muted-foreground py-12">
        No sets match “{{ trimmedFilter }}”.
      </p>

      <div v-else class="space-y-10">
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
    </template>
  </div>
</template>
