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
// parent holds fixed by keying each block on it. Until that first page loads the block shows a
// loading row rather than an empty grid + pagination, matching the pre-split "no flash".
const props = defineProps<{
  game: string
  id: string
  sectionKey: ProductCardSectionKey
  title: string
  blurb: string
}>()

const game = toRef(props, 'game')
const id = toRef(props, 'id')

const page = ref(1)
// The block is reused across product-to-product navigation (its `sectionKey` is held fixed
// by the parent's `:key`), so reset to page 1 when the product changes — otherwise page 3 of
// product A would carry into product B.
watch(id, () => {
  page.value = 1
})

const query = useProductCardsQuery(game, id, page, props.sectionKey)
const cards = computed<Card[]>(() => (query.data.value?.data ?? []).map((entry) => entry.card))
// This section's own card count drives the page count — always the total of the very page set
// being shown, so pagination can never point past the data (keepPreviousData holds the prior
// total across page changes). Absent only before the first page resolves.
const total = computed(() => query.data.value?.total ?? 0)
// Gate the grid + pagination on the first page having loaded (keepPreviousData keeps `data`
// populated across later page changes, so this is only ever true on the initial fetch).
const loaded = computed(() => !query.isPending.value)

// Owned-count badges for signed-in users, over this page's cards (empty otherwise).
const { ownership } = useOwnedCounts(game, cards)
</script>

<template>
  <div>
    <div class="mb-3">
      <h3 class="text-sm font-medium">{{ title }}</h3>
      <p class="text-muted-foreground text-xs">{{ blurb }}</p>
    </div>
    <template v-if="loaded">
      <CardGrid :game="game" :cards="cards" :ownership="ownership" />
      <div class="mt-6">
        <CardPagination v-model:page="page" :page-size="PRODUCT_CARDS_PAGE_SIZE" :total="total" />
      </div>
    </template>
    <LoadingRow v-else label="Loading cards…" />
  </div>
</template>
