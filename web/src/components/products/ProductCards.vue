<script setup lang="ts">
import { computed, toRef } from 'vue'
import type { ProductCardSectionKey } from '@/lib/api'
import { useProductCardSectionsQuery } from '@/composables/useProducts'
import { boosterFamilyLabel } from '@/lib/productType'
import ProductCardsSection from '@/components/products/ProductCardsSection.vue'

// The reverse of the card-detail "Sealed products" section: the cards this sealed product
// is found to contain (decks / promos / Secret Lair), can be pulled from (boosters), or
// may include (randomized products). The API splits them into display sections — guaranteed
// cards, then the family-exclusive booster cards (a collector booster's special printings),
// then the shared booster pool, then maybes — reported by a manifest (which sections exist +
// their counts); each renders as its own **independently paginated** block (issue #224).
// Renders nothing when the product has no ingested contents.
const props = defineProps<{ game: string; id: string; productType: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

const sectionsQuery = useProductCardSectionsQuery(game, id)
const manifest = computed(() => sectionsQuery.data.value?.data ?? [])

// The grand total across sections backs the "Cards in this product (N)" heading.
const total = computed(() => manifest.value.reduce((sum, section) => sum + section.total, 0))

// This booster's family label ("Collector Booster", …), or null for a non-booster product —
// it names the exclusives section and is absent when there's no such section.
const familyLabel = computed(() => boosterFamilyLabel(props.productType))

// Heading + one-line note on how strong the "is in this product" claim is, per section key.
// The exclusive section is named after this product's own booster family.
function sectionMeta(key: string): { title: string; blurb: string } {
  switch (key) {
    case 'contains':
      return { title: 'In the box', blurb: 'Cards guaranteed to be included.' }
    case 'exclusive':
      return {
        title: familyLabel.value ? `${familyLabel.value} exclusives` : 'Booster exclusives',
        blurb: "Cards you can only pull from this product — not the set's other boosters.",
      }
    case 'booster':
      return {
        title: 'Can be pulled from boosters',
        blurb: 'Cards you can open from this product (a random pull).',
      }
    case 'variable':
      return {
        title: 'May be included',
        blurb: 'Cards this product sometimes includes (a randomized configuration).',
      }
    default:
      return { title: key, blurb: '' }
  }
}

// The sections to render, in the manifest's (display) order, each dressed with its heading.
// Each block owns its own paged query (and thus its own card count + pagination), so only the
// key + labels are threaded down; the manifest counts feed the grand total above.
const sections = computed(() =>
  manifest.value.map((section) => ({
    key: section.key as ProductCardSectionKey,
    ...sectionMeta(section.key),
  })),
)
</script>

<template>
  <section v-if="total > 0" class="mt-10">
    <h2 class="mb-4 text-sm font-semibold">
      Cards in this product
      <span class="text-muted-foreground font-normal"> ({{ total.toLocaleString() }})</span>
    </h2>
    <div class="space-y-8">
      <ProductCardsSection
        v-for="section in sections"
        :key="section.key"
        :game="game"
        :id="id"
        :section-key="section.key"
        :title="section.title"
        :blurb="section.blurb"
      />
    </div>
  </section>
</template>
