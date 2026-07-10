<script setup lang="ts">
import { computed, reactive, toRef, watch } from 'vue'
import { ChevronDown } from '@lucide/vue'
import type { SealedProductRef } from '@/lib/api'
import { useCardSealedQuery } from '@/composables/useProducts'
import ProductGrid from '@/components/products/ProductGrid.vue'

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
  <section v-if="sections.length" class="mt-10">
    <h2 class="mb-4 text-sm font-semibold">Sealed products</h2>
    <div class="space-y-8">
      <div v-for="section in sections" :key="section.key">
        <button
          type="button"
          class="group -mx-1.5 mb-3 flex items-start gap-1.5 rounded-md px-1.5 py-1 text-left"
          :aria-expanded="!!expanded[section.key]"
          @click="expanded[section.key] = !expanded[section.key]"
        >
          <ChevronDown
            class="text-muted-foreground group-hover:text-foreground mt-0.5 size-4 shrink-0 transition-transform"
            :class="expanded[section.key] ? 'rotate-180' : ''"
          />
          <span>
            <h3 class="text-sm font-medium">
              {{ section.title }}
              <span class="text-muted-foreground font-normal">({{ section.products.length }})</span>
            </h3>
            <p class="text-muted-foreground text-xs">{{ section.blurb }}</p>
          </span>
        </button>
        <ProductGrid v-if="expanded[section.key]" :game="game" :products="section.products" />
      </div>
    </div>
  </section>
</template>
