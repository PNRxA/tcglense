<script setup lang="ts">
import { computed, toRef } from 'vue'
import { RouterLink } from 'vue-router'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import CardTile from '@/components/cards/CardTile.vue'
import CardGridSkeleton from '@/components/cards/CardGridSkeleton.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import { CARD_PAGE_SIZE, useGameName } from '@/composables/useCatalog'
import {
  usePublicCollectionQuery,
  usePublicCollectionSummaryQuery,
} from '@/composables/usePublicCollection'
import { COLLECTION_DEFAULT_SORT, COLLECTION_SORT_OPTIONS } from '@/lib/cardSort'
import { useCardSearch } from '@/composables/useCardSearch'
import { formatUsd } from '@/lib/money'
import { usePageMeta } from '@/lib/seo'

// A read-only view of a user's public collection for a game (issues #361/#362), addressed
// by their handle. Unauthenticated and indexable. Search + pagination over the owned cards;
// each tile links to the (public) card page. A 404 (private/unknown handle or game) renders
// the not-found state.
const props = defineProps<{ handle: string; game: string }>()
const handle = toRef(props, 'handle')
const game = toRef(props, 'game')
const gameName = useGameName(game)

// The owner's display handle is the username part of the URL handle (`alice-0001` → `alice`).
const username = computed(() => props.handle.replace(/-\d{1,4}$/, ''))

const validSorts = COLLECTION_SORT_OPTIONS.map((option) => option.value)
const { page, searchInput, query, sort } = useCardSearch(COLLECTION_DEFAULT_SORT, validSorts)

const summaryQuery = usePublicCollectionSummaryQuery(handle, game)
const summary = computed(() => summaryQuery.data.value)
const collectionQuery = usePublicCollectionQuery(handle, game, page, query, sort)
const entries = computed(() => collectionQuery.data.value?.data ?? [])
const total = computed(() => collectionQuery.data.value?.total ?? 0)
const notFound = computed(() => collectionQuery.isError.value)

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
      <p class="text-muted-foreground mt-2">
        This collection is private or doesn't exist.
      </p>
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
        <dl v-if="summary" class="mt-4 flex flex-wrap gap-x-8 gap-y-3">
          <div>
            <dt class="text-muted-foreground text-xs tracking-wide uppercase">Unique cards</dt>
            <dd class="text-xl font-semibold tabular-nums">
              {{ summary.unique_cards.toLocaleString() }}
            </dd>
          </div>
          <div>
            <dt class="text-muted-foreground text-xs tracking-wide uppercase">Total copies</dt>
            <dd class="text-xl font-semibold tabular-nums">
              {{ summary.total_cards.toLocaleString() }}
            </dd>
          </div>
          <div v-if="formatUsd(summary.total_value_usd)">
            <dt class="text-muted-foreground text-xs tracking-wide uppercase">Total value</dt>
            <dd class="text-xl font-semibold tabular-nums">
              {{ formatUsd(summary.total_value_usd) }}
            </dd>
          </div>
        </dl>
      </header>

      <StickySearchBar class="mb-6">
        <CardSearchBox
          v-model="searchInput"
          placeholder="Search this collection — name, c:r, t:goblin…"
          class="w-full sm:w-96"
        />
      </StickySearchBar>

      <CardGridSkeleton v-if="collectionQuery.isPending.value" />

      <template v-else>
        <p class="text-muted-foreground mb-4 text-sm">
          {{ total.toLocaleString() }} {{ total === 1 ? 'card' : 'cards' }}
          <template v-if="query"> matching “{{ query }}”</template>
        </p>

        <p v-if="!entries.length && query" class="text-muted-foreground py-12">
          No cards match “{{ query }}”.
        </p>
        <p v-else-if="!entries.length" class="text-muted-foreground py-12">
          This collection is empty.
        </p>

        <template v-else>
          <div
            class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
          >
            <div v-for="entry in entries" :key="entry.card.id" class="relative">
              <CardTile :game="game" :card="entry.card" />
              <span
                class="bg-primary text-primary-foreground absolute top-1 right-1 rounded-full px-1.5 py-0.5 text-xs font-medium tabular-nums shadow"
              >
                ×{{ entry.quantity + entry.foil_quantity }}
              </span>
            </div>
          </div>

          <div class="mt-10">
            <CardPagination v-model:page="page" :page-size="CARD_PAGE_SIZE" :total="total" />
          </div>
        </template>
      </template>
    </template>
  </div>
</template>
