<script setup lang="ts">
import { computed, toRef } from 'vue'
import { RouterLink } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import UpdatingOverlay from '@/components/cards/UpdatingOverlay.vue'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardGridSkeleton from '@/components/cards/CardGridSkeleton.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import AdvancedSearchPanel from '@/components/cards/AdvancedSearchPanel.vue'
import DropSection from '@/components/cards/DropSection.vue'
import GhostToggle from '@/components/cards/GhostToggle.vue'
import GroupViewToggle from '@/components/cards/GroupViewToggle.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import SearchSyntaxHint from '@/components/cards/SearchSyntaxHint.vue'
import SetScopeBar from '@/components/cards/SetScopeBar.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import CollectionGrid from '@/components/collection/CollectionGrid.vue'
import { CARD_PAGE_SIZE } from '@/composables/useCatalog'
import { useHoldingsBrowse } from '@/composables/useHoldingsBrowse'
import {
  usePublicWishlistDropsQuery,
  usePublicWishlistOwnedCounts,
  usePublicWishlistQuery,
  usePublicWishlistSubtypesQuery,
  usePublicWishlistSummaryQuery,
} from '@/composables/usePublicWishlist'

// The read-only card list of a user's public wish list (issue #493): either every wanted card
// (`/u/:handle/wishlist/:game/cards`) or scoped to one set (`.../wishlist/:game/sets/:code`).
// It drives the SAME shared engine as the authed `WishlistBrowseView` by binding the handle into
// the token-less public query hooks via closures (`publicRead: true`). The full authed control
// set comes for free — only the grids render READ-ONLY (a static wanted badge, no editor). A 404
// (private/unknown handle or game) renders the not-found state. The wish-list twin of
// PublicCollectionBrowseView.
const props = defineProps<{ handle: string; game: string; code?: string }>()
const handle = toRef(props, 'handle')
const game = toRef(props, 'game')
// The owner's display handle is the username part of the URL handle (`alice-0001` → `alice`).
const username = computed(() => props.handle.replace(/-\d{1,4}$/, ''))

const {
  game: gameRef,
  code,
  scoped,
  gameName,
  showGhosts,
  setShowGhosts,
  relatedCount,
  hasRelated,
  includeRelated,
  groupMode,
  grouped,
  groupLabel,
  setsWord,
  scopeBarProps,
  setIncludeRelated,
  viewSingleSet,
  setGroupView,
  heading,
  page,
  searchInput,
  query,
  sort,
  sortOptions,
  entries,
  groups,
  ghostCards,
  ghostGroups,
  ownership,
  ownershipReady,
  scopeTotalValue,
  scopeCopiesLabel,
  total,
  listPending,
  listIsError,
  searchError,
  updating,
  hasCards,
  listIsFetching,
  groupPageSize,
  resultsTop,
  countLabel,
  searchPlaceholder,
  loadingLabel,
  errorMessage,
} = useHoldingsBrowse(props, {
  basePath: `/u/${props.handle}/wishlist`,
  publicRead: true,
  useListQuery: (g, p, q, s, setCode, opts) =>
    usePublicWishlistQuery(handle, g, p, q, s, setCode, opts),
  useDropsQuery: (g, c, p, q, opts) => usePublicWishlistDropsQuery(handle, g, c, p, q, opts),
  useSubtypesQuery: (g, c, p, q, opts) => usePublicWishlistSubtypesQuery(handle, g, c, p, q, opts),
  useSummaryQuery: (g, setCode, opts) => usePublicWishlistSummaryQuery(handle, g, setCode, opts),
  useCounts: (g, cards) => usePublicWishlistOwnedCounts(handle, g, cards),
  copy: {
    title: ({ scoped: isScoped, setName, gameName: name }) =>
      isScoped
        ? `${setName} — ${username.value}'s ${name} wish list`
        : `${username.value}'s ${name} wish list`,
    description: ({ gameName: name }) =>
      `${username.value}'s public ${name} wish list on TCGLense.`,
    ownSearchPlaceholder: 'Search this wish list — name, c:r, t:goblin…',
    ownLoadingLabel: 'Loading cards…',
    ownErrorMessage: "Couldn't load this wish list. Please retry.",
  },
})

// `notFound` gates the whole view on the handle-scoped (all-cards) summary — it 404s for a
// private/unknown handle or game regardless of scope. Same key as the engine's own all-cards
// summary read, so this dedupes to a single request.
const summaryQuery = usePublicWishlistSummaryQuery(handle, game)
const notFound = computed(() => summaryQuery.isError.value)
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <div v-if="notFound" class="py-20 text-center">
      <h1 class="text-2xl font-semibold tracking-tight">Wish list not found</h1>
      <p class="text-muted-foreground mt-2">This wish list is private or doesn't exist.</p>
      <RouterLink to="/" class="text-primary mt-4 inline-block underline underline-offset-2">
        Go home
      </RouterLink>
    </div>

    <template v-else>
      <PageBreadcrumbs
        :items="[
          { label: `@${username}`, to: `/u/${handle}` },
          { label: `${gameName} wish list`, to: `/u/${handle}/wishlist/${gameRef}` },
          { label: heading },
        ]"
      />

      <header class="mb-4">
        <h1 class="text-3xl font-semibold tracking-tight">
          <template v-if="scoped">{{ heading }}</template>
          <template v-else>{{ username }}'s {{ gameName }} wish list</template>
        </h1>
        <p class="text-muted-foreground mt-1 text-sm">
          <template v-if="scoped && includeRelated">
            {{ relatedCount }} related {{ setsWord }} ·
          </template>
          <template v-else-if="scoped">
            <span class="uppercase">{{ code }}</span> ·
          </template>
          <template v-if="updating">
            <UpdatingCue />
          </template>
          <template v-else>{{ countLabel }}</template>
          <template v-if="scopeCopiesLabel"> · {{ scopeCopiesLabel }}</template>
          <template v-if="scopeTotalValue">
            ·
            <span class="text-muted-foreground text-[0.7rem] tracking-wide uppercase">Total</span>
            {{ scopeTotalValue }}
          </template>
        </p>
      </header>

      <StickySearchBar>
        <div class="flex items-center gap-2">
          <CardSearchBox v-model="searchInput" :placeholder="searchPlaceholder" class="flex-1" />
          <AdvancedSearchPanel v-model="searchInput" />
        </div>
      </StickySearchBar>
      <SearchSyntaxHint class="mt-2 mb-6" />

      <SetScopeBar
        v-if="hasRelated"
        v-bind="scopeBarProps"
        @toggle="setIncludeRelated"
        @select="viewSingleSet"
      />

      <CardGridSkeleton v-if="listPending" />
      <p v-else-if="listIsError" class="text-destructive py-12">
        {{ searchError ?? errorMessage }}
      </p>

      <template v-else>
        <div
          ref="resultsTop"
          class="mb-4 flex scroll-mt-24 flex-wrap items-center justify-between gap-3"
        >
          <div class="flex flex-wrap items-center gap-2">
            <GroupViewToggle
              v-if="scoped && groupMode"
              :grouped="grouped"
              :label="groupLabel"
              @select="setGroupView"
            />
            <GhostToggle :show-ghosts="showGhosts" @toggle="setShowGhosts" />
          </div>
          <div v-if="hasCards" class="flex gap-2">
            <CardSizeMenu />
            <CardSortMenu v-if="!grouped" v-model="sort" :options="sortOptions" />
          </div>
        </div>

        <LoadingRow v-if="!hasCards && listIsFetching" :label="loadingLabel" />

        <p v-else-if="!hasCards && query" class="text-muted-foreground py-12">
          No cards match “{{ query }}”.
        </p>

        <div v-else-if="!hasCards" class="py-16 text-center">
          <template v-if="showGhosts">
            <p class="text-muted-foreground">
              <template v-if="scoped">No cards in {{ heading }}.</template>
              <template v-else>No {{ gameName }} cards found.</template>
            </p>
          </template>
          <template v-else>
            <p class="text-muted-foreground">
              <template v-if="scoped">{{ username }} wants no cards from {{ heading }}.</template>
              <template v-else>{{ username }}'s {{ gameName }} wish list is empty.</template>
            </p>
            <button
              type="button"
              :class="buttonVariants({ variant: 'default' })"
              class="mt-4 inline-flex"
              @click="setShowGhosts(true)"
            >
              Show all cards
            </button>
          </template>
        </div>

        <template v-else-if="grouped">
          <div class="mb-6">
            <CardPagination
              v-model:page="page"
              :page-size="groupPageSize"
              :total="total"
              :scroll-target="resultsTop"
            />
          </div>
          <UpdatingOverlay :loading="updating">
            <template v-if="!showGhosts">
              <DropSection
                v-for="cardGroup in groups"
                :key="`${code}:${cardGroup.slug ?? cardGroup.title}`"
                :drop="cardGroup"
              >
                <CollectionGrid :game="gameRef" :entries="cardGroup.cards" readonly />
              </DropSection>
            </template>
            <template v-else>
              <DropSection
                v-for="cardGroup in ghostGroups"
                :key="`${code}:${cardGroup.slug ?? cardGroup.title}`"
                :drop="cardGroup"
              >
                <CardGrid
                  :game="gameRef"
                  :cards="cardGroup.cards"
                  :ownership="ownership"
                  :ghost-unowned="ownershipReady"
                  readonly
                />
              </DropSection>
            </template>
          </UpdatingOverlay>
          <div class="mt-10">
            <CardPagination
              v-model:page="page"
              :page-size="groupPageSize"
              :total="total"
              :scroll-target="resultsTop"
            />
          </div>
        </template>

        <template v-else>
          <div class="mb-6">
            <CardPagination
              v-model:page="page"
              :page-size="CARD_PAGE_SIZE"
              :total="total"
              :scroll-target="resultsTop"
            />
          </div>
          <UpdatingOverlay :loading="updating">
            <CollectionGrid v-if="!showGhosts" :game="gameRef" :entries="entries" readonly />
            <CardGrid
              v-else
              :game="gameRef"
              :cards="ghostCards"
              :ownership="ownership"
              :ghost-unowned="ownershipReady"
              readonly
            />
          </UpdatingOverlay>
          <div class="mt-10">
            <CardPagination
              v-model:page="page"
              :page-size="CARD_PAGE_SIZE"
              :total="total"
              :scroll-target="resultsTop"
            />
          </div>
        </template>
      </template>
    </template>
  </div>
</template>
