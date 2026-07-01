<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { LayoutGrid, Library } from '@lucide/vue'
import { RouterLink, useRoute } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CollectionGrid from '@/components/cards/CollectionGrid.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import { useGameName } from '@/composables/useCatalog'
import { useClampPage } from '@/composables/useClampPage'
import {
  COLLECTION_PAGE_SIZE,
  useCollectionQuery,
  useCollectionSummaryQuery,
} from '@/composables/useCollection'
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

const page = ref(1)
// Switching games (the route reuses this component) starts back at page 1.
watch(game, () => {
  page.value = 1
})

const collectionQuery = useCollectionQuery(game, page)
const summaryQuery = useCollectionSummaryQuery(game)

const entries = computed(() => collectionQuery.data.value?.data ?? [])
const total = computed(() => collectionQuery.data.value?.total ?? 0)
const summary = computed(() => summaryQuery.data.value)

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
      </header>

      <LoadingRow v-if="collectionQuery.isPending.value" label="Loading your collection…" />
      <p v-else-if="collectionQuery.isError.value" class="text-destructive py-12">
        Couldn't load your collection. Please retry.
      </p>

      <!-- Empty state: nothing owned yet. -->
      <div v-else-if="!entries.length" class="py-16 text-center">
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

      <template v-else>
        <div class="mb-4 flex justify-end">
          <CardSizeMenu />
        </div>
        <CollectionGrid :game="game" :entries="entries" />
        <div class="mt-10">
          <CardPagination v-model:page="page" :page-size="COLLECTION_PAGE_SIZE" :total="total" />
        </div>
      </template>
    </template>
  </div>
</template>
