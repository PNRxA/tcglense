<script setup lang="ts">
import { computed, toRef } from 'vue'
import { RouterLink } from 'vue-router'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import CardTile from '@/components/cards/CardTile.vue'
import CardGridSkeleton from '@/components/cards/CardGridSkeleton.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import AdvancedSearchPanel from '@/components/cards/AdvancedSearchPanel.vue'
import SearchSyntaxHint from '@/components/cards/SearchSyntaxHint.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import { CARD_PAGE_SIZE, useGameName, useSetQuery } from '@/composables/useCatalog'
import { usePublicCollectionQuery } from '@/composables/usePublicCollection'
import { COLLECTION_DEFAULT_SORT, COLLECTION_SORT_OPTIONS } from '@/lib/cardSort'
import { useCardSearch } from '@/composables/useCardSearch'
import { usePageMeta } from '@/lib/seo'

// The read-only card list of a user's public collection (issues #361/#362): either every
// owned card (`/u/:handle/:game/cards`) or scoped to one set (`/u/:handle/:game/sets/:code`)
// — the two routes share this view, differing only by `code`. Search + pagination; each
// tile links to the (public) card page. A 404 (private/unknown) renders the not-found state.
const props = defineProps<{ handle: string; game: string; code?: string }>()
const handle = toRef(props, 'handle')
const game = toRef(props, 'game')
const gameName = useGameName(game)
const username = computed(() => props.handle.replace(/-\d{1,4}$/, ''))

const scoped = computed(() => !!props.code)
const groupCode = computed(() => props.code ?? '')
const setCode = computed(() => props.code || undefined)
// A set's display name for the header/breadcrumb (public catalog, cached); falls back to
// the upper-cased code until it loads. Only fetched for the set-scoped route.
const setQuery = useSetQuery(game, groupCode, scoped)
const setName = computed(() =>
  scoped.value ? (setQuery.data.value?.name ?? props.code?.toUpperCase() ?? '') : '',
)
const heading = computed(() => (scoped.value ? setName.value : 'All cards'))

const validSorts = COLLECTION_SORT_OPTIONS.map((option) => option.value)
const { page, searchInput, query, sort } = useCardSearch(COLLECTION_DEFAULT_SORT, validSorts)

const collectionQuery = usePublicCollectionQuery(handle, game, page, query, sort, setCode)
const entries = computed(() => collectionQuery.data.value?.data ?? [])
const total = computed(() => collectionQuery.data.value?.total ?? 0)
const notFound = computed(() => collectionQuery.isError.value)

usePageMeta({
  title: () =>
    scoped.value
      ? `${setName.value} — ${username.value}'s ${gameName.value}`
      : `${username.value}'s ${gameName.value} cards`,
  canonicalPath: () =>
    scoped.value
      ? `/u/${handle.value}/${game.value}/sets/${props.code}`
      : `/u/${handle.value}/${game.value}/cards`,
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
        :items="[
          { label: `@${username}`, to: `/u/${handle}` },
          { label: gameName, to: `/u/${handle}/${game}` },
          { label: heading },
        ]"
      />

      <header class="mb-6">
        <h1 class="text-3xl font-semibold tracking-tight">
          <template v-if="scoped">{{ heading }}</template>
          <template v-else>{{ username }}'s {{ gameName }} cards</template>
        </h1>
      </header>

      <!-- Search + the advanced-filter panel (a UI for the Scryfall-style syntax), the
           same pair the catalog and authed collection browse views use. -->
      <StickySearchBar>
        <div class="flex items-center gap-2">
          <CardSearchBox
            v-model="searchInput"
            placeholder="Search this collection — name, c:r, t:goblin…"
            class="flex-1"
          />
          <AdvancedSearchPanel v-model="searchInput" />
        </div>
      </StickySearchBar>
      <SearchSyntaxHint class="mt-2 mb-6" />

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
          No cards to show here.
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
