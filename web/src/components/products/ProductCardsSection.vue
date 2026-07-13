<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { ChevronDown } from '@lucide/vue'
import type { Card, ProductCardSectionKey } from '@/lib/api'
import { PRODUCT_CARDS_PAGE_SIZE, useProductCardsQuery } from '@/composables/useProducts'
import { useOwnedCounts } from '@/composables/useCollection'
import { useWishlistCounts } from '@/composables/useWishlist'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import UpdatingOverlay from '@/components/cards/UpdatingOverlay.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'

// One independently-paginated block of a sealed product's "Cards in this product" section
// (issue #224): a collapsible heading + blurb (collapsed by default, issue #291), this
// section's page of cards, and its own prev/next. Cards (and the page count) come from a
// per-section paged query keyed on `sectionKey`, which the parent holds fixed by keying each
// block on it. The parent's `search` is threaded in so the block pages only the matching
// cards (issue #222). Once expanded, until that first page loads the block shows a loading
// row rather than an empty grid + pagination, matching the pre-split "no flash".
const props = defineProps<{
  game: string
  id: string
  sectionKey: ProductCardSectionKey
  title: string
  blurb: string
  // This section's card count from the parent's manifest — shown on the header so a
  // collapsed block (the default, issue #291) still says how many cards it hides,
  // without needing this block's own paged query (which stays off until expanded).
  count: number
  // The committed card search shared across the product's sections (issue #222). The parent
  // only renders sections the filtered manifest still lists, but a search change races the
  // manifest refetch, so the block also self-hides when it resolves to zero matches (below).
  search: string
  // The committed card sort shared across the product's sections (a `field:dir` option value,
  // or the `default` sentinel for the product's natural order). Threaded into this block's
  // query so every section re-orders together; the manifest is sort-independent (a sort never
  // changes which sections exist or their counts), so only the paged cards move.
  sort: string
}>()

const game = toRef(props, 'game')
const id = toRef(props, 'id')
const search = toRef(props, 'search')
const sort = toRef(props, 'sort')

const page = ref(1)
// This block's own root — paging scrolls it to the top so a section deep in a stack of
// them (a sealed product's card sections) jumps to *its* heading, not the whole page (#258).
const sectionTop = ref<HTMLElement | null>(null)
// The block is reused across product-to-product navigation (its `sectionKey` is held fixed by
// the parent's `:key`) and across search/sort changes, so reset to page 1 whenever the product,
// the search, or the sort changes — otherwise page 3 of the old list would carry into the new.
watch([id, search, sort], () => {
  page.value = 1
})

// Collapsed by default (issue #291): the header (title + count + blurb) stays as a toggle
// and the paged query holds off until expanded, so a product page stacking several big
// sections doesn't render — or fetch — hundreds of cards nobody asked to see. Same idiom
// as SetGroup: an active search auto-reveals (additive only, never force-collapses —
// issue #149's rationale), since a section the filtered manifest still lists holds matches
// the user just searched for. Product-to-product navigation re-collapses (unless arriving
// with a search already committed, e.g. a `?q=` deep link).
const expanded = ref(false)
watch(
  search,
  (q) => {
    if (q) expanded.value = true
  },
  { immediate: true },
)
watch(id, () => {
  expanded.value = search.value.length > 0
})

const query = useProductCardsQuery(game, id, page, props.sectionKey, search, sort, expanded)
const cards = computed<Card[]>(() => (query.data.value?.data ?? []).map((entry) => entry.card))
// This section's own card count drives the page count — always the total of the very page set
// being shown, so pagination can never point past the data (keepPreviousData holds the prior
// total across page changes). Absent only before the first page resolves.
const total = computed(() => query.data.value?.total ?? 0)
// Gate the grid + pagination on the first page having loaded (keepPreviousData keeps `data`
// populated across later page + search changes, so this is only ever true on the initial fetch).
const loaded = computed(() => !query.isPending.value)
// True while paging to (or searching into) a not-yet-loaded page — drives the pagination
// spinner (issue #223) and keeps the current page held up meanwhile (keepPreviousData).
const paging = computed(() => query.isPlaceholderData.value)
// Once loaded, a section with no matches (a search filtered every card out) collapses entirely
// rather than leaving a bare heading — the steady state comes from the filtered manifest, this
// just covers the brief window where a search change outruns the manifest refetch.
const show = computed(() => !loaded.value || total.value > 0)

// Owned-count badges for signed-in users, over this page's cards (empty otherwise).
const { ownership } = useOwnedCounts(game, cards)
// Wish-list wanted counts for the same cards — a Heart chip flags wishlisted cards (#364).
const { ownership: wishlistOwnership } = useWishlistCounts(game, cards)
</script>

<template>
  <div v-if="show" ref="sectionTop" class="scroll-mt-6">
    <button
      type="button"
      class="group -mx-1.5 mb-3 flex items-start gap-1.5 rounded-md px-1.5 py-1 text-left"
      :aria-expanded="expanded"
      @click="expanded = !expanded"
    >
      <ChevronDown
        class="text-muted-foreground group-hover:text-foreground mt-0.5 size-4 shrink-0 transition-transform"
        :class="expanded ? 'rotate-180' : ''"
      />
      <span>
        <h3 class="text-sm font-medium">
          {{ title }}
          <span class="text-muted-foreground font-normal">({{ count.toLocaleString() }})</span>
        </h3>
        <p class="text-muted-foreground text-xs">{{ blurb }}</p>
      </span>
    </button>
    <template v-if="expanded">
      <template v-if="loaded">
        <!-- Top pager mirrors the one below (#264) so a long section can be paged from its top too. -->
        <div class="mb-4">
          <CardPagination
            v-model:page="page"
            :page-size="PRODUCT_CARDS_PAGE_SIZE"
            :total="total"
            :loading="paging"
            :scroll-target="sectionTop"
          />
        </div>
        <UpdatingOverlay :loading="paging">
          <CardGrid
            :game="game"
            :cards="cards"
            :ownership="ownership"
            :wishlist="wishlistOwnership"
          />
        </UpdatingOverlay>
        <div class="mt-6">
          <CardPagination
            v-model:page="page"
            :page-size="PRODUCT_CARDS_PAGE_SIZE"
            :total="total"
            :loading="paging"
            :scroll-target="sectionTop"
          />
        </div>
      </template>
      <LoadingRow v-else label="Loading cards…" />
    </template>
  </div>
</template>
