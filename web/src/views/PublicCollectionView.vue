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
import { useGameName } from '@/composables/useCatalog'
import { useHoldingsLanding } from '@/composables/useHoldingsLanding'
import {
  usePublicCollectionSetsQuery,
  usePublicCollectionSummaryQuery,
} from '@/composables/usePublicCollection'
import { usePageMeta } from '@/lib/seo'

// A user's public collection landing for a game (issues #361/#362): the sets they own
// cards in (grouped + filterable, with per-set owned counts/values) plus a "View all
// cards" link. Read-only and indexable — it drives the *same* `useHoldingsLanding` engine
// as the authed collection landing, fed public (token-less) summary/sets queries. A 404
// (private/unknown handle or game) renders the not-found state.
const props = defineProps<{ handle: string; game: string }>()
const handle = toRef(props, 'handle')
const gameName = useGameName(toRef(props, 'game'))
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
  bulkValue,
  hasStats,
} = useHoldingsLanding(props, {
  useSummaryQuery: (g) => usePublicCollectionSummaryQuery(handle, g),
  useHeldSetsQuery: (g) => usePublicCollectionSetsQuery(handle, g),
  basePath: `/u/${props.handle}`,
  withBulk: true,
})

// The landing's `activeError` flips with the inherited `?sets=all` scope toggle (it then
// reads the always-200 public catalog list), so gate not-found on the handle-scoped summary
// query instead — it 404s for a private/unknown handle regardless of the toggle. Same key as
// the engine's own summary read, so this dedupes to a single request.
const summaryQuery = usePublicCollectionSummaryQuery(handle, game)
const notFound = computed(() => summaryQuery.isError.value)

usePageMeta({
  title: () => `${username.value}'s ${gameName.value} collection`,
  description: () => `${username.value}'s public ${gameName.value} collection on TCGLense.`,
  canonicalPath: () => `/u/${handle.value}/${game.value}`,
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <div v-if="notFound" class="py-20 text-center">
      <h1 class="text-2xl font-semibold tracking-tight">Collection not found</h1>
      <p class="text-muted-foreground mt-2">This collection is private or doesn't exist.</p>
      <RouterLink to="/" class="text-primary mt-4 inline-block underline underline-offset-2">
        Go home
      </RouterLink>
    </div>

    <template v-else>
      <PageBreadcrumbs
        :items="[{ label: `@${username}`, to: `/u/${handle}` }, { label: gameName }]"
      />

      <header class="mb-6">
        <h1 class="text-3xl font-semibold tracking-tight">
          {{ username }}'s {{ gameName }} collection
        </h1>
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
          <div v-if="bulkValue">
            <dt class="text-muted-foreground text-xs tracking-wide uppercase">Bulk value</dt>
            <dd class="text-xl font-semibold tabular-nums">{{ bulkValue }}</dd>
          </div>
        </dl>
      </header>

      <StickySearchBar class="mb-6 flex flex-wrap items-center gap-3">
        <CardSearchBox
          v-if="sourceSets.length"
          v-model="filter"
          class="w-full sm:w-64"
          aria-label="Filter sets by name or code"
          placeholder="Filter sets…"
        />
        <RouterLink
          :to="`/u/${handle}/${game}/cards`"
          :class="buttonVariants({ variant: 'default' })"
          class="shrink-0"
        >
          <LayoutGrid />
          View all cards
        </RouterLink>
      </StickySearchBar>

      <SetGridSkeleton v-if="activePending" />
      <p v-else-if="!sourceSets.length" class="text-muted-foreground py-12">
        This collection is empty.
      </p>
      <p v-else-if="filtering && !groups.length" class="text-muted-foreground py-12">
        No sets match “{{ trimmedFilter }}”.
      </p>
      <!-- Owned sets (grouped, with nested sub-sets), each tile carrying the owner's counts
           and linking to that set's public card view. -->
      <SetGroupGrid
        v-else
        :game="game"
        :groups="groups"
        :scroll-mt="28"
        :base-path="`/u/${handle}`"
        :query="trimmedFilter"
        :ownership="ownership"
      />
    </template>
  </div>
</template>
