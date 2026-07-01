<script setup lang="ts">
import { computed, toRef } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { RouterLink } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import CollectionGrid from '@/components/cards/CollectionGrid.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import SearchSyntaxHint from '@/components/cards/SearchSyntaxHint.vue'
import CollectionSignInPrompt from '@/components/collection/CollectionSignInPrompt.vue'
import { searchErrorMessage, useCardSearch } from '@/composables/useCardSearch'
import { useGameName } from '@/composables/useCatalog'
import { useClampPage } from '@/composables/useClampPage'
import { COLLECTION_PAGE_SIZE, useCollectionQuery } from '@/composables/useCollection'
import { COLLECTION_DEFAULT_SORT, COLLECTION_SORT_OPTIONS } from '@/lib/cardSort'
import { getSet } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'

// Owned cards for a game, either the whole collection (`/collection/:game/cards`) or
// scoped to one set (`/collection/:game/sets/:code`). The two routes share this view;
// `code` is the only difference (undefined = all cards), mirroring the catalog's
// CardsBrowseView / SetView split against one collection.
const props = defineProps<{ game: string; code?: string }>()
const game = toRef(props, 'game')
const code = toRef(props, 'code')
const setCode = computed(() => props.code || undefined)
const scoped = computed(() => !!setCode.value)

const gameName = useGameName(game)
const auth = useAuthStore()

// A set's display name for the header/breadcrumb (public, cached). Only fetched for the
// set-scoped view; falls back to the upper-cased code until it loads or if it's unknown.
const setQuery = useQuery({
  queryKey: ['set', game, code],
  queryFn: () => getSet(game.value, code.value as string),
  enabled: scoped,
})
const setName = computed(() =>
  scoped.value ? (setQuery.data.value?.name ?? code.value?.toUpperCase() ?? '') : '',
)
const heading = computed(() => (scoped.value ? setName.value : 'All cards'))

// Per-account page — kept out of search indexes.
usePageMeta({
  title: () =>
    scoped.value
      ? `${setName.value} — your ${gameName.value} collection`
      : `All your ${gameName.value} cards`,
  canonicalPath: () =>
    scoped.value
      ? `/collection/${game.value}/sets/${code.value}`
      : `/collection/${game.value}/cards`,
  noindex: true,
})

// Page, search and sort live in the URL query (like the catalog browse views), so they
// survive opening a card and pressing Back and are shareable/reload-safe.
const { page, searchInput, query, sort } = useCardSearch(
  COLLECTION_DEFAULT_SORT,
  COLLECTION_SORT_OPTIONS.map((option) => option.value),
)

const collectionQuery = useCollectionQuery(game, page, query, sort, setCode)
const entries = computed(() => collectionQuery.data.value?.data ?? [])
const total = computed(() => collectionQuery.data.value?.total ?? 0)
// A malformed search query comes back as 422; surface its message inline.
const searchError = computed(() => searchErrorMessage(collectionQuery.error.value))

useClampPage(page, () => ({
  ready: collectionQuery.isSuccess.value,
  total: total.value,
  pageSize: COLLECTION_PAGE_SIZE,
}))

const countLabel = computed(() => {
  const label = `${total.value.toLocaleString()} ${total.value === 1 ? 'card' : 'cards'}`
  return query.value ? `${label} matching “${query.value}”` : label
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <nav class="text-muted-foreground mb-4 text-sm">
      <RouterLink to="/collection" class="hover:underline">Collection</RouterLink>
      <span class="mx-1.5">/</span>
      <RouterLink :to="`/collection/${game}`" class="hover:underline">{{ gameName }}</RouterLink>
      <span class="mx-1.5">/</span>
      <span class="text-foreground">{{ heading }}</span>
    </nav>

    <!-- Signed out: the collection routes are public, so prompt to sign in rather than
         bouncing to the login page (matches the landing view, preserving ?redirect). -->
    <CollectionSignInPrompt v-if="!auth.isAuthenticated" :game-name="gameName" />

    <template v-else>
      <header class="mb-4">
        <h1 class="text-3xl font-semibold tracking-tight">{{ heading }}</h1>
        <p class="text-muted-foreground mt-1 text-sm">
          <template v-if="scoped">
            <span class="uppercase">{{ code }}</span> ·
          </template>
          {{ countLabel }}
        </p>
      </header>

      <!-- Search + sort over the (optionally set-scoped) owned cards. -->
      <div class="bg-background/85 sticky top-0 z-30 -mx-4 border-b px-4 py-3 backdrop-blur">
        <CardSearchBox
          v-model="searchInput"
          :placeholder="
            scoped
              ? 'Search this set — name, c:r, t:goblin…'
              : 'Search your collection — name, c:r, t:goblin…'
          "
        />
      </div>
      <SearchSyntaxHint class="mt-2 mb-6" />

      <LoadingRow v-if="collectionQuery.isPending.value" label="Loading your cards…" />
      <p v-else-if="collectionQuery.isError.value" class="text-destructive py-12">
        {{ searchError ?? "Couldn't load your collection. Please retry." }}
      </p>

      <!-- A search that matched nothing. -->
      <p v-else-if="!entries.length && query" class="text-muted-foreground py-12">
        No cards match “{{ query }}”.
      </p>

      <!-- No entries but a fetch is still in flight (e.g. clearing a zero-match search:
           keepPreviousData holds the empty page while the unscoped list reloads). Keep a
           loading affordance rather than flashing the "empty" state below. -->
      <LoadingRow
        v-else-if="!entries.length && collectionQuery.isFetching.value"
        label="Loading your cards…"
      />

      <!-- Nothing owned in this scope (e.g. a direct link to a set you own nothing in). -->
      <div v-else-if="!entries.length" class="py-16 text-center">
        <p class="text-muted-foreground">
          <template v-if="scoped">You don't own any cards from {{ heading }} yet.</template>
          <template v-else>Your {{ gameName }} collection is empty.</template>
        </p>
        <RouterLink
          :to="scoped ? `/cards/${game}/sets/${code}` : `/cards/${game}/cards`"
          :class="buttonVariants({ variant: 'default' })"
          class="mt-4 inline-flex"
        >
          Browse cards to add some
        </RouterLink>
      </div>

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
