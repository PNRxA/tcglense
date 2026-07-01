<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { useQueryClient } from '@tanstack/vue-query'
import { LayoutGrid, Library, RefreshCw } from '@lucide/vue'
import { RouterLink, useRoute } from 'vue-router'
import { Button, buttonVariants } from '@/components/ui/button'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import CollectionGrid from '@/components/cards/CollectionGrid.vue'
import ImportCollectionDialog from '@/components/collection/ImportCollectionDialog.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import SearchSyntaxHint from '@/components/cards/SearchSyntaxHint.vue'
import { searchErrorMessage, useCardSearch } from '@/composables/useCardSearch'
import { useGameName } from '@/composables/useCatalog'
import { useClampPage } from '@/composables/useClampPage'
import { COLLECTION_DEFAULT_SORT, COLLECTION_SORT_OPTIONS } from '@/lib/cardSort'
import {
  COLLECTION_PAGE_SIZE,
  invalidateCollectionData,
  useCollectionQuery,
  useCollectionSourceQuery,
  useCollectionSummaryQuery,
  useImportJobQuery,
  useSyncCollectionSourceMutation,
} from '@/composables/useCollection'
import { ApiError } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'

const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')
const gameName = useGameName(game)

const auth = useAuthStore()
const route = useRoute()
// After signing in / up, come back to this collection (both forms honour ?redirect).
const loginTo = computed(() => ({ path: '/login', query: { redirect: route.fullPath } }))
const registerTo = computed(() => ({ path: '/register', query: { redirect: route.fullPath } }))

// Per-account page — kept out of search indexes.
usePageMeta({
  title: () => `Your ${gameName.value} collection`,
  canonicalPath: () => `/collection/${game.value}`,
  noindex: true,
})

// Page, search and sort live in the URL query (like the catalog browse views), so
// they survive opening a card and pressing Back and are shareable/reload-safe.
// Switching games routes to a fresh path, which resets them.
const { page, searchInput, query, sort } = useCardSearch(
  COLLECTION_DEFAULT_SORT,
  COLLECTION_SORT_OPTIONS.map((option) => option.value),
)

const collectionQuery = useCollectionQuery(game, page, query, sort)
const summaryQuery = useCollectionSummaryQuery(game)

const entries = computed(() => collectionQuery.data.value?.data ?? [])
const total = computed(() => collectionQuery.data.value?.total ?? 0)
const summary = computed(() => summaryQuery.data.value)
// A malformed search query comes back as 422; surface its message inline.
const searchError = computed(() => searchErrorMessage(collectionQuery.error.value))

// Keep the requested page within range as the collection shrinks (e.g. after
// removing the last card on the final page).
useClampPage(page, () => ({
  ready: collectionQuery.isSuccess.value,
  total: total.value,
  pageSize: COLLECTION_PAGE_SIZE,
}))

const totalValue = computed(() => {
  const raw = summary.value?.total_value_usd
  if (!raw) return null
  const n = Number(raw)
  return Number.isFinite(n)
    ? n.toLocaleString(undefined, { style: 'currency', currency: 'USD' })
    : `$${raw}`
})

// Stats are worth showing only once something is owned.
const hasStats = computed(() => (summary.value?.unique_cards ?? 0) > 0)

// Whether the collection is genuinely empty — decided by the whole-collection summary
// (which no search filters), so a zero-match search or an in-flight refetch never makes
// a non-empty collection look empty. Wait for the summary to load before deciding.
const collectionIsEmpty = computed(() => summaryQuery.isSuccess.value && !hasStats.value)

// Show the search + sort controls whenever the collection has cards or a search is
// active; a genuinely empty collection keeps the clean "add some cards" CTA instead.
const showControls = computed(() => hasStats.value || !!query.value)

// Import / sync from an external collection provider (Archidekt today).
const qc = useQueryClient()
const sourceQuery = useCollectionSourceQuery(game)
const source = computed(() => sourceQuery.data.value ?? null)
const syncMutation = useSyncCollectionSourceMutation()
const syncMessage = ref<string | null>(null)

// Re-sync runs in the background (throttled by the provider rate limit); poll its job.
const syncJobId = ref<number | null>(null)
const syncJobQuery = useImportJobQuery(game, syncJobId)
const syncStatus = computed(() => syncJobQuery.data.value?.status ?? null)
const syncing = computed(
  () =>
    syncMutation.isPending.value || syncStatus.value === 'queued' || syncStatus.value === 'running',
)

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
  syncJobId.value = null
  try {
    const job = await syncMutation.mutateAsync({ game: game.value })
    syncJobId.value = job.job_id
  } catch (err) {
    syncMessage.value = err instanceof ApiError ? err.message : 'Re-sync failed. Please try again.'
  }
}

// React to the polled re-sync job finishing.
watch(
  () => syncJobQuery.data.value,
  (job) => {
    if (!job) return
    if (job.status === 'running') {
      syncMessage.value = smart.value
        ? 'Smart-syncing from Archidekt… this can take a couple of minutes.'
        : 'Re-syncing from Archidekt… this can take a couple of minutes.'
    } else if (job.status === 'complete') {
      const s = job.summary
      if (!s) {
        syncMessage.value = 'Re-sync complete.'
      } else if (s.mode === 'smart') {
        syncMessage.value =
          `Smart-synced ${s.matched_cards.toLocaleString()} cards` +
          (s.stopped_early ? ' (stopped at already-synced cards).' : '.')
      } else {
        syncMessage.value =
          `Synced ${s.matched_cards.toLocaleString()} cards` +
          (s.removed_cards ? `, removed ${s.removed_cards.toLocaleString()}.` : '.')
      }
      invalidateCollectionData(qc, game.value)
      qc.invalidateQueries({ queryKey: ['collection-source', game.value] })
    } else if (job.status === 'error') {
      syncMessage.value = job.error ?? 'Re-sync failed. Please try again.'
    }
  },
)
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
    <div v-if="!auth.isAuthenticated" class="mx-auto max-w-md py-16 text-center">
      <div class="bg-muted mx-auto flex size-12 items-center justify-center rounded-lg">
        <Library class="size-6" aria-hidden="true" />
      </div>
      <h1 class="mt-4 text-2xl font-semibold tracking-tight">Sign in to view your collection</h1>
      <p class="text-muted-foreground mt-2">
        Track which {{ gameName }} cards you own and what they're worth. Sign in or create a free
        account to start your collection.
      </p>
      <div class="mt-6 flex justify-center gap-3">
        <RouterLink :to="loginTo" :class="buttonVariants({ variant: 'default' })">
          Sign in
        </RouterLink>
        <RouterLink :to="registerTo" :class="buttonVariants({ variant: 'outline' })">
          Create account
        </RouterLink>
      </div>
    </div>

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

      <!-- Search + sort over the collection (same Scryfall syntax as the catalog).
           Shown once the collection has cards, or while a search is active. -->
      <template v-if="showControls">
        <div class="bg-background/85 sticky top-0 z-30 -mx-4 border-b px-4 py-3 backdrop-blur">
          <CardSearchBox
            v-model="searchInput"
            placeholder="Search your collection — name, c:r, t:goblin…"
          />
        </div>
        <SearchSyntaxHint class="mt-2" />
        <p v-if="query" class="text-muted-foreground mt-4 mb-6 text-sm">
          <template v-if="collectionQuery.isFetching.value && !entries.length">Searching…</template>
          <template v-else>
            {{ total.toLocaleString() }} {{ total === 1 ? 'card' : 'cards' }} matching “{{ query }}”
          </template>
        </p>
      </template>

      <LoadingRow v-if="collectionQuery.isPending.value" label="Loading your collection…" />
      <p v-else-if="collectionQuery.isError.value" class="text-destructive py-12">
        {{ searchError ?? "Couldn't load your collection. Please retry." }}
      </p>

      <!-- Genuinely-empty collection (no search active): prompt to add cards. Gated on
           the summary, not the filtered list, so an in-flight search-clear never shows
           this by mistake. -->
      <div v-else-if="collectionIsEmpty && !query" class="py-16 text-center">
        <p class="text-muted-foreground">Your {{ gameName }} collection is empty.</p>
        <RouterLink
          :to="`/cards/${game}/cards`"
          :class="buttonVariants({ variant: 'default' })"
          class="mt-4 inline-flex"
        >
          <LayoutGrid />
          Browse cards to add some
        </RouterLink>
      </div>

      <!-- A search that matched nothing in the collection. -->
      <p v-else-if="!entries.length && query" class="text-muted-foreground py-12">
        No cards in your collection match “{{ query }}”.
      </p>

      <!-- No entries yet but the collection isn't empty (e.g. refetching after clearing
           a search): keep a loading affordance rather than flashing an empty state. -->
      <LoadingRow v-else-if="!entries.length" label="Loading your collection…" />

      <template v-else>
        <div class="mb-4 flex justify-end gap-2">
          <CardSizeMenu />
          <CardSortMenu v-model="sort" :options="COLLECTION_SORT_OPTIONS" />
        </div>
        <CollectionGrid :game="game" :entries="entries" />
        <div class="mt-10">
          <CardPagination v-model:page="page" :page-size="COLLECTION_PAGE_SIZE" :total="total" />
        </div>
      </template>
    </template>
  </div>
</template>
