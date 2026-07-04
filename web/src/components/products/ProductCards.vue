<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import type { Card, ProductCardEntry } from '@/lib/api'
import { PRODUCT_CARDS_PAGE_SIZE, useProductCardsQuery } from '@/composables/useProducts'
import { useOwnedCounts } from '@/composables/useCollection'
import { boosterFamilyLabel } from '@/lib/productType'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardPagination from '@/components/cards/CardPagination.vue'

// The reverse of the card-detail "Sealed products" section: the cards this sealed product
// is found to contain (decks / promos / Secret Lair), can be pulled from (boosters), or
// may include (randomized products) — the three membership buckets, guaranteed cards
// first, then the wider booster pool (issue #204). Within the booster pool the cards
// exclusive to this booster family (a collector booster's special printings that no other
// booster in the set can pull) lead their own section. Paginated by card. Renders nothing
// when the product has no ingested contents, so a product with no card data adds nothing.
const props = defineProps<{ game: string; id: string; productType: string }>()
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

// This booster's family label ("Collector Booster", …), or null for a non-booster product
// — it names the exclusives section and is absent when there's no such section.
const familyLabel = computed(() => boosterFamilyLabel(props.productType))

// The buckets in display order, each with a heading + one-line note on how strong the "is
// in this product" claim is: guaranteed cards, then the family-exclusive booster cards
// (this booster's special printings), then the shared booster pool, then maybes. Entries
// arrive globally ordered to match, so filtering the current page by bucket in this order
// reproduces the on-page order. The exclusive bucket only fills for a booster product
// (the API flags no card `exclusive` otherwise), so its section is self-hiding when empty.
const sections = computed(() => {
  const groups = [
    {
      key: 'contains',
      title: 'In the box',
      blurb: 'Cards guaranteed to be included.',
      match: (e: ProductCardEntry) => e.membership === 'contains',
    },
    {
      key: 'exclusive',
      title: familyLabel.value ? `${familyLabel.value} exclusives` : 'Booster exclusives',
      blurb: "Cards you can only pull from this product — not the set's other boosters.",
      match: (e: ProductCardEntry) => e.membership === 'booster' && e.exclusive,
    },
    {
      key: 'booster',
      title: 'Can be pulled from boosters',
      blurb: 'Cards you can open from this product (a random pull).',
      match: (e: ProductCardEntry) => e.membership === 'booster' && !e.exclusive,
    },
    {
      key: 'variable',
      title: 'May be included',
      blurb: 'Cards this product sometimes includes (a randomized configuration).',
      match: (e: ProductCardEntry) => e.membership === 'variable',
    },
  ]
  return groups
    .map((group) => ({
      ...group,
      cards: entries.value.filter(group.match).map((entry) => entry.card),
    }))
    .filter((section) => section.cards.length > 0)
})

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
