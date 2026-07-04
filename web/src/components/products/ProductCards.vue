<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import type { Card, ProductCardEntry } from '@/lib/api'
import { PRODUCT_CARDS_PAGE_SIZE, useProductCardsQuery } from '@/composables/useProducts'
import { useOwnedCounts } from '@/composables/useCollection'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardPagination from '@/components/cards/CardPagination.vue'

// The reverse of the card-detail "Sealed products" section: the cards this sealed product
// is found to contain (decks / promos / Secret Lair), can be pulled from (boosters), or
// may include (randomized products) — the three membership buckets, guaranteed cards
// first, then the wider booster pool (issue #204). Paginated by card. Renders nothing
// when the product has no ingested contents, so a product with no card data adds nothing.
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

const page = ref(1)
// vue-router reuses this component across product-to-product navigation (the RouterView
// has no per-route key, and a card modal's "Sealed products" links jump between product
// pages), so reset to page 1 when the product changes — otherwise page 3 of product A
// would carry over to product B.
watch(id, () => {
  page.value = 1
})
const query = useProductCardsQuery(game, id, page)

const entries = computed<ProductCardEntry[]>(() => query.data.value?.data ?? [])
const total = computed(() => query.data.value?.total ?? 0)

// The three buckets in display order, each with a heading + one-line note on how strong
// the "is in this product" claim is. Entries arrive globally ordered by membership rank,
// so filtering the current page by bucket in this order reproduces the on-page order.
const GROUPS = [
  { key: 'contains', title: 'In the box', blurb: 'Cards guaranteed to be included.' },
  {
    key: 'booster',
    title: 'Can be pulled from boosters',
    blurb: 'Cards you can open from this product (a random pull).',
  },
  {
    key: 'variable',
    title: 'May be included',
    blurb: 'Cards this product sometimes includes (a randomized configuration).',
  },
] as const

// Only buckets with at least one card on the current page, so an absent bucket renders no
// heading. A bucket that spans a page boundary simply repeats its heading on the next page.
const sections = computed(() =>
  GROUPS.map((group) => ({
    ...group,
    cards: entries.value
      .filter((entry) => entry.membership === group.key)
      .map((entry) => entry.card),
  })).filter((section) => section.cards.length > 0),
)

// Owned-count badges for signed-in users, over every card on the page (empty otherwise).
const pageCards = computed<Card[]>(() => entries.value.map((entry) => entry.card))
const { ownership } = useOwnedCounts(game, pageCards)

// Gate on a known-positive total so an empty product shows nothing and there's no flash
// (keepPreviousData keeps the grid up across page changes), mirroring CardSealedProducts.
const hasCards = computed(() => total.value > 0)
</script>

<template>
  <section v-if="hasCards" class="mt-10">
    <h2 class="mb-4 text-sm font-semibold">
      Cards in this product
      <span class="text-muted-foreground font-normal"> ({{ total.toLocaleString() }})</span>
    </h2>
    <div class="space-y-8">
      <div v-for="section in sections" :key="section.key">
        <div class="mb-3">
          <h3 class="text-sm font-medium">{{ section.title }}</h3>
          <p class="text-muted-foreground text-xs">{{ section.blurb }}</p>
        </div>
        <CardGrid :game="game" :cards="section.cards" :ownership="ownership" />
      </div>
    </div>
    <div class="mt-8">
      <CardPagination v-model:page="page" :page-size="PRODUCT_CARDS_PAGE_SIZE" :total="total" />
    </div>
  </section>
</template>
