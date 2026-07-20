<script setup lang="ts">
import { computed, reactive, toRef, watch } from 'vue'
import type { SealedProductRef } from '@/lib/api'
import { useCardSealedQuery } from '@/composables/useProducts'
import { useCollectionProductCounts } from '@/composables/useCollection'
import { useWishlistProductCounts } from '@/composables/useWishlist'
import ProductGrid from '@/components/products/ProductGrid.vue'
import CollapsibleSection from '@/components/shared/CollapsibleSection.vue'

// The card-detail "Sealed products" section: which sealed products this card is found
// in (decks / promos / Secret Lair), can be pulled from (boosters), or may be in
// (randomized products) — the three membership buckets the API returns. Renders nothing
// when the card is in no ingested product, so a card with no sealed data adds nothing to
// the page. Shown in both the full card page and the browse-grid modal (both mount
// CardDetailContent).
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

const query = useCardSealedQuery(game, id)
const refs = computed<SealedProductRef[]>(() => query.data.value?.data ?? [])

// Resting wanted-count badges for the sealed-product tiles' quick-add controls: one batch
// POST for every product this card appears in (across all buckets), keyed by product id so
// each bucket grid reads only its own. Signed out returns `{}` and the grid renders no
// controls.
const products = computed(() => refs.value.map((r) => r.product))
const { ownership: wanted } = useWishlistProductCounts(game, products)
const { ownership: owned } = useCollectionProductCounts(game, products)

// The three buckets in display order, each with a heading + one-line explanation of how
// strong the "is in this product" claim is.
const GROUPS = [
  {
    key: 'contains',
    title: 'Found in',
    blurb: 'Sealed products guaranteed to contain this card.',
  },
  {
    key: 'booster',
    title: 'Can be pulled from',
    blurb: 'Booster products this card can be opened from (a random pull).',
  },
  {
    key: 'variable',
    title: 'May be in',
    blurb: 'Randomized products that sometimes include this card.',
  },
] as const

// Only buckets with at least one product, so an absent bucket renders no heading.
const sections = computed(() =>
  GROUPS.map((group) => ({
    ...group,
    products: refs.value.filter((r) => r.membership === group.key).map((r) => r.product),
  })).filter((section) => section.products.length > 0),
)

// Each bucket collapses independently (issue #332), matching the sealed product page's
// card sections — collapsed by default with a count on the heading, so a card in dozens of
// products doesn't stack grid after grid until asked. Keyed by the bucket key (an absent
// key reads as collapsed); card-to-card navigation re-collapses every bucket.
const expanded = reactive<Record<string, boolean>>({})
watch(id, () => {
  for (const key of Object.keys(expanded)) expanded[key] = false
})
</script>

<template>
  <section v-if="sections.length">
    <h2 class="mb-3 text-base font-semibold tracking-tight">Sealed products</h2>
    <div class="space-y-3">
      <CollapsibleSection
        v-for="section in sections"
        :key="section.key"
        v-model:expanded="expanded[section.key]"
        :title="section.title"
        :count="section.products.length"
        :blurb="section.blurb"
      >
        <ProductGrid :game="game" :products="section.products" :owned="owned" :wanted="wanted" />
      </CollapsibleSection>
    </div>
  </section>
</template>
