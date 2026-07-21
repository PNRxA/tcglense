<script setup lang="ts">
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import UpdatingOverlay from '@/components/cards/UpdatingOverlay.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import { buttonVariants } from '@/components/ui/button'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardGridSkeleton from '@/components/cards/CardGridSkeleton.vue'
import GhostToggle from '@/components/cards/GhostToggle.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import AdvancedSearchPanel from '@/components/cards/AdvancedSearchPanel.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import CollectionGrid from '@/components/collection/CollectionGrid.vue'
import DropSection from '@/components/cards/DropSection.vue'
import GroupViewToggle from '@/components/cards/GroupViewToggle.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import SearchSyntaxHint from '@/components/cards/SearchSyntaxHint.vue'
import SetScopeBar from '@/components/cards/SetScopeBar.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import CollectionSignInPrompt from '@/components/collection/CollectionSignInPrompt.vue'
import { CARD_PAGE_SIZE } from '@/composables/useCatalog'
import {
  useCollectionDropsQuery,
  useCollectionQuery,
  useCollectionSubtypesQuery,
  useCollectionSummaryQuery,
  useOwnedCounts,
} from '@/composables/useCollection'
import { useHoldingsBrowse } from '@/composables/useHoldingsBrowse'
import { useAuthStore } from '@/stores/auth'

// Owned cards for a game, either the whole collection (`/collection/:game/cards`) or
// scoped to one set (`/collection/:game/sets/:code`). The two routes share this view;
// `code` is the only difference (undefined = all cards), mirroring the catalog's
// CardsBrowseView / SetView split against one collection.
//
// The entire reactive engine — the {owned, ghost} × {flat, by-drop} data-source matrix, the
// show-ghosts / by-drop / include-related view controls, and every derived label — is shared
// with WishlistBrowseView through `useHoldingsBrowse`; only this template (which renders the
// bulk-value slice and passes no wish-list `list`/owned-marks props) differs.
const props = defineProps<{ game: string; code?: string }>()
const auth = useAuthStore()

const {
  game,
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
  wishlistCounts,
  scopeTotalValue,
  scopeBulkValue,
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
  basePath: '/collection',
  useListQuery: useCollectionQuery,
  useDropsQuery: useCollectionDropsQuery,
  useSubtypesQuery: useCollectionSubtypesQuery,
  useSummaryQuery: useCollectionSummaryQuery,
  useCounts: useOwnedCounts,
  enableWishlistHearts: true,
  copy: {
    title: ({ scoped, setName, gameName }) =>
      scoped ? `${setName} — your ${gameName} collection` : `All your ${gameName} cards`,
    ownSearchPlaceholder: 'Search your collection — name, c:r, t:goblin…',
    ownLoadingLabel: 'Loading your cards…',
    ownErrorMessage: "Couldn't load your collection. Please retry.",
  },
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <PageBreadcrumbs
      :items="[
        { label: 'Collection', to: '/collection' },
        { label: gameName, to: `/collection/${game}` },
        { label: heading },
      ]"
    />

    <!-- Signed out (session resolved): prompt to sign in rather than bouncing to the login
         page (matches the landing view, preserving ?redirect). While the initial session is
         still resolving, show the pending grid instead of flashing the prompt at a returning
         signed-in visitor. -->
    <CollectionSignInPrompt
      v-if="auth.sessionResolved && !auth.isAuthenticated"
      :game-name="gameName"
    />

    <template v-else-if="auth.isAuthenticated">
      <header class="mb-4">
        <h1 class="text-3xl font-semibold tracking-tight">{{ heading }}</h1>
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
          <!-- The scope's total copies (with duplicates), shown when you own more copies
               than distinct cards (issue #125). Hidden while searching. -->
          <template v-if="scopeCopiesLabel"> · {{ scopeCopiesLabel }}</template>
          <!-- The scope's owned value (issue #119): what your cards in this set / group /
               whole collection are worth — the total, then its bulk (< $1/card) slice to the
               right. Hidden while searching or when nothing is priced. -->
          <template v-if="scopeTotalValue">
            ·
            <span class="text-muted-foreground text-[0.7rem] tracking-wide uppercase">Total</span>
            {{ scopeTotalValue }}
          </template>
          <template v-if="scopeBulkValue">
            ·
            <span class="text-muted-foreground text-[0.7rem] tracking-wide uppercase">Bulk</span>
            {{ scopeBulkValue }}
          </template>
        </p>
      </header>

      <!-- Search + sort over the (optionally set-scoped) cards. -->
      <StickySearchBar>
        <div class="flex items-center gap-2">
          <CardSearchBox v-model="searchInput" :placeholder="searchPlaceholder" class="flex-1" />
          <AdvancedSearchPanel v-model="searchInput" />
        </div>
      </StickySearchBar>
      <SearchSyntaxHint class="mt-2 mb-6" />

      <!-- Fold the set's related sub-sets (tokens, promos, decks, …) into one listing, plus
           a picker to drop into any single set — the collection mirror of the catalog set
           view's scope bar. Composes with show-ghosts; acting on it leaves by-drop. -->
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
        <!-- Controls: the grouped / all-cards toggle (grouped sets: By drop or By
             treatment) + the show-ghosts toggle, then the card-size + sort menus (both
             views — picking a sort from a grouped view flips to the flat all-cards grid,
             since a grouped view has a fixed order). -->
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
            <CardSortMenu v-model="sort" :options="sortOptions" />
          </div>
        </div>

        <!-- No cards yet but a fetch is still in flight (e.g. clearing a zero-match search:
             keepPreviousData holds the empty page while the list reloads). Keep a loading
             affordance rather than flashing an "empty" state. -->
        <LoadingRow v-if="!hasCards && listIsFetching" :label="loadingLabel" />

        <!-- A search that matched nothing. -->
        <p v-else-if="!hasCards && query" class="text-muted-foreground py-12">
          No cards match “{{ query }}”.
        </p>

        <!-- Nothing to show. In show-ghosts mode that means the catalog has no cards in
             scope; otherwise it's an empty (sub-)collection, so offer to switch ghosts
             on — the full card list in scope with unowned cards dimmed, whose add-in-place
             controls write to the collection (mirrors the wish-list browse view). -->
        <div v-else-if="!hasCards" class="py-16 text-center">
          <template v-if="showGhosts">
            <p class="text-muted-foreground">
              <template v-if="scoped">No cards in {{ heading }} yet.</template>
              <template v-else>No {{ gameName }} cards found.</template>
            </p>
          </template>
          <template v-else>
            <p class="text-muted-foreground">
              <template v-if="scoped">You don't own any cards from {{ heading }} yet.</template>
              <template v-else>Your {{ gameName }} collection is empty.</template>
            </p>
            <button
              type="button"
              :class="buttonVariants({ variant: 'default' })"
              class="mt-4 inline-flex"
              @click="setShowGhosts(true)"
            >
              Show all cards to add some
            </button>
          </template>
        </div>

        <!-- Grouped: one section per group (Secret Lair drop or card sub-type), paginated
             by group. Owned mode uses the collection grid (its cards are owned holdings);
             ghost mode uses the catalog grid (every card in the group) with owned badges +
             dimmed unowned cards. -->
        <template v-else-if="grouped">
          <!-- Top pager mirrors the one below (#264) so a long list can be paged from the top too. -->
          <div class="mb-6">
            <CardPagination
              v-model:page="page"
              :page-size="groupPageSize"
              :total="total"
              :scroll-target="resultsTop"
            />
          </div>
          <!-- Two typed loops (not one union v-for): owned groups render the collection
               grid off owned holdings, ghost groups the catalog grid off every card. -->
          <UpdatingOverlay :loading="updating">
            <template v-if="!showGhosts">
              <DropSection
                v-for="cardGroup in groups"
                :key="`${code}:${cardGroup.slug ?? cardGroup.title}`"
                :drop="cardGroup"
              >
                <CollectionGrid
                  :game="game"
                  :entries="cardGroup.cards"
                  :wishlist="wishlistCounts"
                />
              </DropSection>
            </template>
            <template v-else>
              <DropSection
                v-for="cardGroup in ghostGroups"
                :key="`${code}:${cardGroup.slug ?? cardGroup.title}`"
                :drop="cardGroup"
              >
                <CardGrid
                  :game="game"
                  :cards="cardGroup.cards"
                  :ownership="ownership"
                  :ghost-unowned="ownershipReady"
                  :wishlist="wishlistCounts"
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

        <!-- Flat: one grid. Owned-only mode uses the collection grid (seeded quick-add
             counts); ghost mode uses the catalog grid with owned-count badges + dimmed
             unowned cards. The dim waits for ownership to load (ownershipReady) so owned
             cards don't flash as ghosts on first paint / a page change. -->
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
            <CollectionGrid
              v-if="!showGhosts"
              :game="game"
              :entries="entries"
              :wishlist="wishlistCounts"
            />
            <CardGrid
              v-else
              :game="game"
              :cards="ghostCards"
              :ownership="ownership"
              :ghost-unowned="ownershipReady"
              :wishlist="wishlistCounts"
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

    <!-- Initial session still resolving: reserve the card grid's layout. -->
    <CardGridSkeleton v-else />
  </div>
</template>
