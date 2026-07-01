<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { useQueryClient } from '@tanstack/vue-query'
import { LayoutGrid, RefreshCw } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { Button, buttonVariants } from '@/components/ui/button'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import SetGroupGrid from '@/components/cards/SetGroupGrid.vue'
import CollectionSignInPrompt from '@/components/collection/CollectionSignInPrompt.vue'
import ImportCollectionDialog from '@/components/collection/ImportCollectionDialog.vue'
import QuickAddBox from '@/components/collection/QuickAddBox.vue'
import { useGameName } from '@/composables/useCatalog'
import { useFilteredSetGroups } from '@/composables/useSetGrouping'
import {
  invalidateCollectionData,
  useCollectionSetsQuery,
  useCollectionSummaryQuery,
} from '@/composables/useCollection'
import {
  useCollectionSourceQuery,
  usePolledImportJob,
  useSyncCollectionSourceMutation,
} from '@/composables/useCollectionImport'
import { ApiError } from '@/lib/api'
import { formatUsd } from '@/lib/money'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'

// The per-game collection landing: it mirrors the catalog's game view (a grid of set
// tiles + a "View all cards" entry), but scoped to what the signed-in user owns — pick
// a set to see just your cards from it, or "All cards" for the whole collection. The
// header carries the value/count summary plus the import / re-sync controls; the actual
// card grids live on CollectionBrowseView (`/collection/:game/cards` + `.../sets/:code`).
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
const setsQuery = useCollectionSetsQuery(game)

const summary = computed(() => summaryQuery.data.value)
const ownedSets = computed(() => setsQuery.data.value?.data ?? [])

// Client-side filter box + nested sub-set grouping mirroring the catalog game view
// (issues #127/#128), shared via useFilteredSetGroups: the owned-set list is already in
// memory, so narrowing by name/code is instant, and a sub-set you own nests under its
// owned parent (or stands alone as an orphan root). Keeps a group whole when the main set
// OR any related sub-set matches, so filtering a sub-set surfaces its whole owned group.
const {
  filter,
  trimmedFilter,
  filtering,
  groups: ownedGroups,
  relatedCount,
} = useFilteredSetGroups(game, ownedSets)
// Per-set-code owned stats each tile shows next to its name: the "N/M owned" completion
// count, the "N copies" total (when you own duplicates, issue #125), and the preformatted
// owned value (issue #119; null/unpriced sets carry a null the tile omits). Built in one
// pass and passed to SetGroupGrid as a single `ownership` object.
const ownership = computed(() => {
  const counts: Record<string, number> = {}
  const copies: Record<string, number> = {}
  const values: Record<string, string | null> = {}
  for (const set of ownedSets.value) {
    counts[set.code] = set.owned_cards
    copies[set.code] = set.owned_copies
    values[set.code] = formatUsd(set.owned_value_usd)
  }
  return { counts, copies, values }
})
const totalValue = computed(() => formatUsd(summary.value?.total_value_usd))

// Stats are worth showing only once something is owned.
const hasStats = computed(() => (summary.value?.unique_cards ?? 0) > 0)

// Whether the collection is genuinely empty — decided by the whole-collection summary
// (so an in-flight sets refetch never makes a non-empty collection look empty). Wait for
// the summary to load before deciding.
const collectionIsEmpty = computed(() => summaryQuery.isSuccess.value && !hasStats.value)

// Import / sync from an external collection provider (Archidekt today).
const qc = useQueryClient()
const sourceQuery = useCollectionSourceQuery(game)
const source = computed(() => sourceQuery.data.value ?? null)
const syncMutation = useSyncCollectionSourceMutation()
const syncMessage = ref<string | null>(null)

const providerLabel = computed(() =>
  source.value?.provider === 'archidekt' ? 'Archidekt' : (source.value?.provider ?? 'Archidekt'),
)
// A saved link can re-sync by smart (incremental) sync or a full mirror; the label,
// confirmation, and result copy differ because smart never removes cards.
const smart = computed(() => source.value?.smart ?? false)
const lastSyncedText = computed(() => {
  const t = source.value?.last_synced_at
  if (!t) return 'Not synced yet'
  const d = new Date(t)
  return Number.isNaN(d.getTime()) ? '' : `Last synced ${d.toLocaleString()}`
})

// Re-sync runs in the background (throttled by the provider rate limit); poll its job to a
// terminal status via the shared poller (usePolledImportJob), tailoring the copy to smart
// vs. mirror. On completion, refresh the collection views and the saved-link timestamp.
const syncJob = usePolledImportJob(game, {
  onRunning: () => {
    syncMessage.value = smart.value
      ? 'Smart-syncing from Archidekt… this can take a couple of minutes.'
      : 'Re-syncing from Archidekt… this can take a couple of minutes.'
  },
  onComplete: (summary) => {
    if (!summary) {
      syncMessage.value = 'Re-sync complete.'
    } else if (summary.mode === 'smart') {
      syncMessage.value =
        `Smart-synced ${summary.matched_cards.toLocaleString()} cards` +
        (summary.stopped_early ? ' (stopped at already-synced cards).' : '.')
    } else {
      syncMessage.value =
        `Synced ${summary.matched_cards.toLocaleString()} cards` +
        (summary.removed_cards ? `, removed ${summary.removed_cards.toLocaleString()}.` : '.')
    }
    invalidateCollectionData(qc, game.value)
    qc.invalidateQueries({ queryKey: ['collection-source', game.value] })
  },
  onError: (error) => {
    syncMessage.value = error ?? 'Re-sync failed. Please try again.'
  },
})
const syncing = computed(() => syncMutation.isPending.value || syncJob.processing.value)

// A full re-sync mirrors (replace), so it can remove cards — confirm before running. A
// smart re-sync only updates recently-changed cards and never removes, so it's gentler.
async function resync() {
  if (!source.value || syncing.value) return
  const message = smart.value
    ? `Smart-sync updates recently-changed cards from your ${providerLabel.value} collection ` +
      "(it won't remove cards). Continue?"
    : `Re-syncing replaces your ${gameName.value} collection with your ${providerLabel.value} ` +
      'collection, removing cards that are no longer in it. Continue?'
  const ok = window.confirm(message)
  if (!ok) return
  syncMessage.value = 'Re-sync queued…'
  syncJob.reset()
  try {
    const job = await syncMutation.mutateAsync({ game: game.value })
    syncJob.start(job.job_id)
  } catch (err) {
    syncMessage.value = err instanceof ApiError ? err.message : 'Re-sync failed. Please try again.'
  }
}
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <nav class="text-muted-foreground mb-4 text-sm">
      <RouterLink to="/collection" class="hover:underline">Collection</RouterLink>
      <span class="mx-1.5">/</span>
      <span class="text-foreground">{{ gameName }}</span>
    </nav>

    <!-- Signed out: the collection routes are public, so rather than bouncing to the
         login page we prompt to sign in / sign up right here. -->
    <CollectionSignInPrompt v-if="!auth.isAuthenticated" :game-name="gameName" />

    <template v-else>
      <header class="mb-6">
        <h1 class="text-3xl font-semibold tracking-tight">Your {{ gameName }} collection</h1>

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
            <dt class="text-muted-foreground text-xs tracking-wide uppercase">Est. value</dt>
            <dd class="text-xl font-semibold tabular-nums">{{ totalValue }}</dd>
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
        </div>

        <!-- Import / re-sync from an external collection provider. -->
        <div class="mt-5 flex flex-wrap items-center gap-3">
          <ImportCollectionDialog :game="game" :source="source" />
          <template v-if="source">
            <Button variant="secondary" size="sm" :disabled="syncing" @click="resync">
              <RefreshCw :class="{ 'animate-spin': syncing }" />
              {{ smart ? 'Smart re-sync' : 'Re-sync' }} from {{ providerLabel }}
            </Button>
            <span class="text-muted-foreground text-sm">{{ lastSyncedText }}</span>
          </template>
        </div>
        <p v-if="syncMessage" class="text-muted-foreground mt-2 text-sm" aria-live="polite">
          {{ syncMessage }}
        </p>
      </header>

      <LoadingRow v-if="summaryQuery.isPending.value" label="Loading your collection…" />
      <p v-else-if="summaryQuery.isError.value" class="text-destructive py-12">
        Couldn't load your collection. Please retry.
      </p>

      <!-- Genuinely-empty collection: prompt to add cards. -->
      <div v-else-if="collectionIsEmpty" class="py-16 text-center">
        <p class="text-muted-foreground">Your {{ gameName }} collection is empty.</p>
        <RouterLink
          :to="`/cards/${game}`"
          :class="buttonVariants({ variant: 'default' })"
          class="mt-4 inline-flex"
        >
          <LayoutGrid />
          Browse cards to add some
        </RouterLink>
      </div>

      <!-- Pick a set or view every owned card — mirrors the catalog's game view. -->
      <template v-else>
        <div class="mb-6 flex flex-wrap items-center justify-between gap-3">
          <h2 class="text-xl font-semibold tracking-tight">
            Sets you own
            <!-- Only once the sets query has resolved, so we never flash "0 sets" next
                 to the "Loading sets…" row when the summary lands first. Counts top-level
                 groups (sub-sets nest under their parent), with the folded-in related
                 count alongside — mirroring the catalog game view. -->
            <span
              v-if="setsQuery.isSuccess.value"
              class="text-muted-foreground ml-1 text-sm font-normal"
            >
              {{ ownedGroups.length }} {{ ownedGroups.length === 1 ? 'set' : 'sets' }}
              <template v-if="relatedCount > 0"> · {{ relatedCount }} related</template>
              <template v-if="filtering"> matching “{{ trimmedFilter }}”</template>
            </span>
          </h2>
          <div class="flex flex-wrap items-center gap-3">
            <!-- Instant client-side filter over the owned sets, mirroring the catalog
                 game view (issue #127); only shown once you own at least one set. -->
            <CardSearchBox
              v-if="ownedSets.length"
              v-model="filter"
              class="w-full sm:w-56"
              aria-label="Filter sets by name or code"
              placeholder="Filter sets…"
            />
            <RouterLink
              :to="`/collection/${game}/cards`"
              :class="buttonVariants({ variant: 'default' })"
            >
              <LayoutGrid />
              View all cards
            </RouterLink>
          </div>
        </div>

        <LoadingRow v-if="setsQuery.isPending.value" label="Loading sets…" />
        <p v-else-if="setsQuery.isError.value" class="text-destructive py-12">
          Couldn't load your sets. Please retry.
        </p>
        <p v-else-if="filtering && !ownedGroups.length" class="text-muted-foreground py-12">
          No sets match “{{ trimmedFilter }}”.
        </p>
        <!-- Owned sub-sets nest under their parent (SetGroup), matching the catalog game
             view; a childless owned set stays a plain tile. Both link to the collection's
             per-set view and show owned counts. -->
        <SetGroupGrid
          v-else
          :game="game"
          :groups="ownedGroups"
          :scroll-mt="20"
          base-path="/collection"
          :ownership="ownership"
        />
      </template>
    </template>
  </div>
</template>
