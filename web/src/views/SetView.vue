<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { ArrowLeft, CalendarClock } from '@lucide/vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import UpdatingOverlay from '@/components/cards/UpdatingOverlay.vue'
import { RouterLink } from 'vue-router'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardGridSkeleton from '@/components/cards/CardGridSkeleton.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import AdvancedSearchPanel from '@/components/cards/AdvancedSearchPanel.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import DropSection from '@/components/cards/DropSection.vue'
import GroupViewToggle from '@/components/cards/GroupViewToggle.vue'
import SearchSyntaxHint from '@/components/cards/SearchSyntaxHint.vue'
import SetScopeBar from '@/components/cards/SetScopeBar.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import { searchErrorMessage, useCardSearch, useDropFilter } from '@/composables/useCardSearch'
import {
  CARD_PAGE_SIZE,
  DROP_PAGE_SIZE,
  SUBTYPE_PAGE_SIZE,
  useSetCardsQuery,
  useSetDropsQuery,
  useSetQuery,
  useSetSubtypesQuery,
} from '@/composables/useCatalog'
import { useClampPage } from '@/composables/useClampPage'
import { useCurrency } from '@/composables/useCurrency'
import { useOwnedCounts } from '@/composables/useCollection'
import { useSetGrouping } from '@/composables/useSetGrouping'
import { useWishlistCounts } from '@/composables/useWishlist'
import { SET_DEFAULT_SORT, SET_SORT_OPTIONS } from '@/lib/cardSort'
import { type Card } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'
import { formatReleaseLabel } from '@/lib/releaseDate'

const props = defineProps<{ game: string; code: string }>()
const game = toRef(props, 'game')
const code = toRef(props, 'code')

// Related-sub-set grouping + the "view all together" / "view just one set" scope nav +
// the grouped view (Secret Lair drops or card sub-types), all keyed off the (game-cached)
// full set list. `groupMode`/`grouped` and `setsPending` come from that same list, which
// the flat card fetch below gates on — so a grouped set is known up front (no flat-grid
// flash, no throwaway fetch).
const {
  group,
  relatedCount,
  hasRelated,
  includeRelated,
  groupMode,
  grouped,
  groupLabel,
  setsWord,
  scopeBarProps,
  setsPending,
  setIncludeRelated,
  viewSingleSet,
  setGroupView,
} = useSetGrouping(game, code)

// Page, search and sort live in the URL query (alongside the related/from scope), so
// they survive opening a card and pressing Back. Routing to a different set lands on
// a fresh URL, so it starts clean. Committing a sort while grouped leaves the fixed-order
// grouped view for the flat sorted grid (?view=all), folded into the same write — and sheds
// the by-drop ?drop= filter, matching setGroupView's flat switch (it's inert on the flat grid).
const { page, searchInput, query, sort } = useCardSearch(
  SET_DEFAULT_SORT,
  SET_SORT_OPTIONS.map((option) => option.value),
  () => (grouped.value ? { view: 'all', drop: undefined } : {}),
)

// The grouped view breaks down into either Secret Lair drops or card sub-types (never
// both — see `groupMode`). Split the flag so the drops-only bits (the drop-title filter)
// stay drops-only.
const byDrop = computed(() => grouped.value && groupMode.value === 'drops')
const bySubtype = computed(() => grouped.value && groupMode.value === 'subtypes')

// The by-drop "filter drops by name" box, backed by ?drop= (orthogonal to the card
// search ?q above): q narrows the cards within each drop, this narrows the drops by
// their curated title. Only rendered in the by-drop view; passing `byDrop` lets it
// drop a mid-debounce keystroke when the view leaves by-drop (so a half-typed filter
// can't land a phantom ?drop= on the flat view's URL).
const { dropInput, dropQuery } = useDropFilter(byDrop)

const setQuery = useSetQuery(game, code)

const set = computed(() => setQuery.data.value)

usePageMeta({
  title: () => set.value?.name ?? code.value.toUpperCase(),
  description: () =>
    set.value
      ? `Browse cards from ${set.value.name} on TCGLense, with singles prices tracked over time.`
      : undefined,
  canonicalPath: () => `/cards/${game.value}/sets/${code.value}`,
})

const cardsQuery = useSetCardsQuery(game, code, {
  page,
  query,
  sort,
  defaultSort: SET_DEFAULT_SORT,
  includeRelated,
  // Skip the flat list while a grouped view is active, and wait for the set list to
  // settle first — it's what tells us whether this set is grouped (and resolves the
  // related grouping), so we never fire a throwaway flat request that a cold-loaded
  // grouped / related link would immediately discard.
  enabled: computed(() => !grouped.value && !setsPending.value),
})

// The two grouped data sources — Secret Lair drops and card sub-types. Both hooks are
// called unconditionally (composables can't be conditional); their `enabled` gates pick
// the one matching this set's `groupMode`, so only the active one ever fetches.
const dropsQuery = useSetDropsQuery(game, code, { page, query, drop: dropQuery, enabled: byDrop })
const subtypesQuery = useSetSubtypesQuery(game, code, { page, query, enabled: bySubtype })
// The active grouped query, picked by reference so its reactive fields stay live.
const groupQuery = computed(() => (bySubtype.value ? subtypesQuery : dropsQuery))

const cards = computed(() => cardsQuery.data.value?.data ?? [])
const total = computed(() => cardsQuery.data.value?.total ?? 0)
// The active grouped view's groups (drops or sub-types) and their count. Paginated by
// group, so `groupTotal` is a group count, not a card count.
const groups = computed(() => groupQuery.value.data.value?.data ?? [])
const groupTotal = computed(() => groupQuery.value.data.value?.total ?? 0)
const groupPageSize = computed(() => (bySubtype.value ? SUBTYPE_PAGE_SIZE : DROP_PAGE_SIZE))
// keepPreviousData holds the prior page while the next loads; drives the "Updating…" cue.
const groupLoading = computed(() => groupQuery.value.isPlaceholderData.value)
// The top of the results (the controls row above the grid) — both pagers scroll here so a
// page change starts at the top of the listing, clearing the sticky search bar (issue #258).
const resultsTop = ref<HTMLElement | null>(null)

// Every card visible on the current page — the flat grid's cards, or all the groups'
// cards in a grouped view — so a single owned-counts lookup drives the collection
// badges on whichever grid(s) render below.
const visibleCards = computed<Card[]>(() =>
  grouped.value ? groups.value.flatMap((group) => group.cards) : cards.value,
)
const { ownership } = useOwnedCounts(game, visibleCards)
// Wish-list wanted counts for the same cards — a Heart chip flags wishlisted cards (#364).
const { ownership: wishlistOwnership } = useWishlistCounts(game, visibleCards)

// Each by-drop header shows the drop's "cheapest prints" total — for each card in the drop,
// its cheapest printing anywhere in the catalog, summed server-side
// (DropGroup.cheapest_prints_usd) — rendered in the viewer's display currency (null when
// nothing in the drop is priced). Read straight off the drops query (only drops carry the
// field), which in the by-drop view *is* `groups`, so the array aligns index-for-index with
// the section v-for below (#456).
const money = useCurrency()
const printsTotals = computed(() =>
  (dropsQuery.data.value?.data ?? []).map((drop) => money.formatUsd(drop.cheapest_prints_usd)),
)

// Each by-drop header also shows the drop's release date (DropGroup.released_at, derived
// server-side from the drop's cards — they share one street date). A future date reads as
// "Releases …" so a freshly-previewed Scryfall drop shows when it's due; a past one as
// "Released …" — the shared `formatReleaseLabel` (short month here). Index-aligned to `groups`
// in the by-drop view, exactly like `printsTotals`.
const releaseDates = computed(() =>
  (dropsQuery.data.value?.data ?? []).map((drop) => formatReleaseLabel(drop.released_at, 'short')),
)

// The list's loading / error / empty state reads from whichever query drives the current
// view. cardsQuery waits on the set list, so an as-yet-undecided grouped set shows the
// active query's own pending state (no flat-grid flash), while keepPreviousData still
// carries the prior set's cards smoothly across navigation.
const listPending = computed(() =>
  grouped.value ? groupQuery.value.isPending.value : cardsQuery.isPending.value,
)
const listError = computed(() =>
  grouped.value ? groupQuery.value.error.value : cardsQuery.error.value,
)
const listIsError = computed(() =>
  grouped.value ? groupQuery.value.isError.value : cardsQuery.isError.value,
)
// Refetching over stale results (page/filter change held by keepPreviousData): drives an
// honest "Updating…" cue on the count line rather than silently showing the old total.
const updating = computed(() =>
  grouped.value
    ? groupQuery.value.isFetching.value && groupLoading.value
    : cardsQuery.isFetching.value && cardsQuery.isPlaceholderData.value,
)
const isEmpty = computed(() =>
  grouped.value ? groups.value.length === 0 : cards.value.length === 0,
)

// The active view sets the pagination unit: groups (grouped) or printings (flat).
useClampPage(page, () =>
  grouped.value
    ? {
        ready: groupQuery.value.isSuccess.value,
        total: groupTotal.value,
        pageSize: groupPageSize.value,
      }
    : { ready: cardsQuery.isSuccess.value, total: total.value, pageSize: CARD_PAGE_SIZE },
)

// When folding in related sets, the page is rooted at the group's main set.
const heading = computed(() =>
  includeRelated.value && group.value
    ? group.value.main.name
    : (set.value?.name ?? code.value.toUpperCase()),
)
const countLabel = computed(() => {
  // A grouped view counts its groups (drops or sub-types); the flat view counts card
  // printings. The by-drop view has two filters — the drop-title box (dropQuery) and the
  // card search (q) — so its "matching …" suffix reflects whichever is active (the drop
  // filter reads first, as it's what the drop count directly narrows).
  const [n, singular] = grouped.value
    ? [groupTotal.value, bySubtype.value ? 'sub-type' : 'drop']
    : [total.value, 'printing']
  const active = byDrop.value ? dropQuery.value || query.value : query.value
  if (!n && !active) return ''
  const label = `${n.toLocaleString()} ${n === 1 ? singular : `${singular}s`}`
  return active ? `${label} matching “${active}”` : label
})
// A malformed search query comes back as 422; surface its message inline.
const searchError = computed(() => searchErrorMessage(listError.value))
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <RouterLink
      :to="`/cards/${game}`"
      class="text-muted-foreground hover:text-foreground mb-4 inline-flex items-center gap-1.5 text-sm"
    >
      <ArrowLeft class="size-4" />
      All sets
    </RouterLink>

    <p v-if="setQuery.isError.value" class="text-destructive py-12">Set not found.</p>

    <template v-else>
      <header class="mb-4">
        <h1 class="text-3xl font-semibold tracking-tight">{{ heading }}</h1>
        <p class="text-muted-foreground mt-1 text-sm">
          <template v-if="includeRelated">{{ relatedCount }} related {{ setsWord }}</template>
          <template v-else>
            <span class="uppercase">{{ code }}</span>
            <template v-if="set?.set_type"> · {{ set?.set_type?.replace('_', ' ') }}</template>
          </template>
          <template v-if="updating"> · <UpdatingCue /> </template>
          <template v-else-if="countLabel"> · {{ countLabel }}</template>
        </p>
      </header>

      <!-- The search bar sticks to the top of the viewport so it stays reachable
           while scrolling a long set. -->
      <StickySearchBar>
        <div class="flex items-center gap-2">
          <CardSearchBox
            v-model="searchInput"
            :placeholder="
              includeRelated ? 'Search these sets — c:r, t:land…' : 'Search this set — c:r, t:land…'
            "
            class="flex-1"
          />
          <AdvancedSearchPanel v-model="searchInput" />
        </div>
      </StickySearchBar>
      <SearchSyntaxHint class="mt-2 mb-6" />

      <!-- Offer folding the set's related sub-sets (tokens, promos, decks, …) into
           one listing instead of visiting each individually. Shown in the by-drop
           view too (a Secret Lair-style set can still have related sub-sets), sitting
           above the by-drop/all-cards toggle — the two controls are orthogonal: this
           scopes across sub-sets, that groups the set's own cards. Folding in related
           sets is inherently a flat cross-set listing, so acting on it leaves by-drop. -->
      <SetScopeBar
        v-if="hasRelated"
        v-bind="scopeBarProps"
        @toggle="setIncludeRelated"
        @select="viewSingleSet"
      />

      <!-- Controls: a grouped / All-cards toggle for grouped sets (By drop or By
           treatment), a card-size menu, and the sort menu. The size and sort menus show in
           both views (the grouped sections are grids too); picking a sort from a grouped
           view flips to the flat all-cards grid (a grouped view has a fixed order — see the
           sort's onSortCommit above). These sit above the results (not inside the has-results
           branch below) so the toggle and its drop-name filter stay visible — and
           clearable — even when the filter narrows to nothing; the size/sort menus only
           make sense with cards on screen, so they hide while the current view is empty. -->
      <div
        v-if="groupMode || !isEmpty"
        ref="resultsTop"
        class="mb-4 flex scroll-mt-24 flex-wrap items-center justify-between gap-3"
      >
        <GroupViewToggle
          v-if="groupMode"
          :grouped="grouped"
          :label="groupLabel"
          @select="setGroupView"
        />
        <span v-else />
        <div v-if="!isEmpty" class="flex gap-2">
          <CardSizeMenu />
          <CardSortMenu v-model="sort" :options="SET_SORT_OPTIONS" />
        </div>
      </div>

      <!-- Filter the drops by their curated Secret Lair title, sitting under the
           By-drop toggle. Server-side (the by-drop view paginates over drops), so it
           narrows the whole set, not just the drops on the current page. Drops-only —
           the by-treatment view has a small fixed set of sub-types, so no name filter. -->
      <div v-if="byDrop" class="mb-6 max-w-sm">
        <CardSearchBox
          v-model="dropInput"
          placeholder="Filter drops by name…"
          aria-label="Filter drops by name"
        />
      </div>

      <CardGridSkeleton v-if="listPending" />
      <p v-else-if="listIsError" class="text-destructive py-12">
        {{ searchError ?? "Couldn't load cards. Please retry." }}
      </p>
      <!-- With both filters active the empty response can't attribute the cause (the card
           search `q` empties drops server-side before the title filter sees them), so name
           neither — just report no match. Each filter alone gets its own precise message. -->
      <p v-else-if="isEmpty && byDrop && dropQuery && query" class="text-muted-foreground py-12">
        No drops match your filters.
      </p>
      <p v-else-if="isEmpty && byDrop && dropQuery" class="text-muted-foreground py-12">
        No drops match “{{ dropQuery }}”.
      </p>
      <p v-else-if="isEmpty && query" class="text-muted-foreground py-12">
        No cards match “{{ query }}”.
      </p>
      <p v-else-if="isEmpty" class="text-muted-foreground py-12">
        No cards in {{ includeRelated ? 'these sets' : 'this set' }} yet.
      </p>

      <template v-else>
        <!-- Grouped: one section per group (Secret Lair drop or card sub-type),
             paginated by group. -->
        <template v-if="grouped">
          <!-- Top pager mirrors the one below (#264) so a long list can be paged from the top too. -->
          <div class="mb-6">
            <CardPagination
              v-model:page="page"
              :page-size="groupPageSize"
              :total="groupTotal"
              :loading="groupLoading"
              :scroll-target="resultsTop"
            />
          </div>
          <UpdatingOverlay :loading="groupLoading">
            <DropSection
              v-for="(cardGroup, i) in groups"
              :key="`${code}:${cardGroup.slug ?? cardGroup.title}`"
              :drop="cardGroup"
            >
              <!-- Drops only: the release date and the cheapest-prints total, right-aligned in
                   the header. Omitted for sub-type groups; each line omits itself when absent
                   (a drop with no date, or nothing priced). -->
              <template v-if="byDrop && (releaseDates[i] || printsTotals[i])" #meta>
                <span class="flex flex-col items-end gap-0.5 text-right">
                  <span
                    v-if="releaseDates[i]"
                    class="inline-flex items-center gap-1 text-sm font-medium"
                    :class="releaseDates[i]?.upcoming ? 'text-primary' : 'text-muted-foreground'"
                  >
                    <CalendarClock class="size-3.5 shrink-0" />
                    {{ releaseDates[i]?.label }}
                  </span>
                  <span v-if="printsTotals[i]" class="text-sm">
                    <span class="text-muted-foreground font-normal">cheapest prints</span>
                    <span class="text-foreground ml-1 font-medium tabular-nums">
                      {{ printsTotals[i] }}
                    </span>
                  </span>
                </span>
              </template>
              <CardGrid
                :game="game"
                :cards="cardGroup.cards"
                :ownership="ownership"
                :wishlist="wishlistOwnership"
              />
            </DropSection>
          </UpdatingOverlay>
          <div class="mt-10">
            <CardPagination
              v-model:page="page"
              :page-size="groupPageSize"
              :total="groupTotal"
              :loading="groupLoading"
              :scroll-target="resultsTop"
            />
          </div>
        </template>

        <!-- Flat: the whole set as one collector-ordered grid. -->
        <template v-else>
          <div class="mb-6">
            <CardPagination
              v-model:page="page"
              :page-size="CARD_PAGE_SIZE"
              :total="total"
              :loading="cardsQuery.isPlaceholderData.value"
              :scroll-target="resultsTop"
            />
          </div>
          <UpdatingOverlay :loading="cardsQuery.isPlaceholderData.value">
            <CardGrid
              :game="game"
              :cards="cards"
              :ownership="ownership"
              :wishlist="wishlistOwnership"
            />
          </UpdatingOverlay>
          <div class="mt-10">
            <CardPagination
              v-model:page="page"
              :page-size="CARD_PAGE_SIZE"
              :total="total"
              :loading="cardsQuery.isPlaceholderData.value"
              :scroll-target="resultsTop"
            />
          </div>
        </template>
      </template>
    </template>
  </div>
</template>
