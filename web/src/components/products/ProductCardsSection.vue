<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import type { Card, ProductCardSectionKey } from '@/lib/api'
import { PRODUCT_CARDS_PAGE_SIZE, useProductCardsQuery } from '@/composables/useProducts'
import { useOwnedCounts } from '@/composables/useCollection'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'

// One independently-paginated block of a sealed product's "Cards in this product" section
// (issue #224): a heading + blurb, this section's page of cards, and its own prev/next. Cards
// (and the page count) come from a per-section paged query keyed on `sectionKey`, which the
// parent holds fixed by keying each block on it. The parent's `search` is threaded in so the
// block pages only the matching cards (issue #222). Until that first page loads the block
// shows a loading row rather than an empty grid + pagination, matching the pre-split "no flash".
const props = defineProps<{
  game: string
  id: string
  sectionKey: ProductCardSectionKey
  title: string
  blurb: string
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

const query = useProductCardsQuery(game, id, page, props.sectionKey, search, sort)
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
</script>

<template>
  <div v-if="show" ref="sectionTop" class="scroll-mt-6">
    <div class="mb-3">
      <h3 class="text-sm font-medium">{{ title }}</h3>
      <p class="text-muted-foreground text-xs">{{ blurb }}</p>
    </div>
    <template v-if="loaded">
      <CardGrid :game="game" :cards="cards" :ownership="ownership" />
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
  </div>
</template>
