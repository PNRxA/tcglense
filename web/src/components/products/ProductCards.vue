<script setup lang="ts">
import { computed, toRef } from 'vue'
import type { ProductCardSectionKey } from '@/lib/api'
import { useProductCardSectionsQuery } from '@/composables/useProducts'
import { useProductCardsSearch } from '@/composables/useProductCardsSearch'
import { searchErrorMessage } from '@/composables/useCardSearch'
import { boosterFamilyLabel } from '@/lib/productType'
import ProductCardsSection from '@/components/products/ProductCardsSection.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import AdvancedSearchPanel from '@/components/cards/AdvancedSearchPanel.vue'
import SearchSyntaxHint from '@/components/cards/SearchSyntaxHint.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import { PRODUCT_CARDS_DEFAULT_SORT, PRODUCT_CARDS_SORT_OPTIONS } from '@/lib/cardSort'

// The reverse of the card-detail "Sealed products" section: the cards this sealed product
// is found to contain (decks / promos / Secret Lair), can be pulled from (boosters), or
// may include (randomized products). The API splits them into display sections — guaranteed
// cards, then the family-exclusive booster cards (a collector booster's special printings),
// then the shared booster pool, then maybes — reported by a manifest (which sections exist +
// their counts); each renders as its own **independently paginated** block (issue #224).
// A search box narrows the whole pool with the catalog's Scryfall-style grammar, filtering
// every section's cards + the manifest live (issue #222). Renders nothing when the product
// has no ingested contents.
const props = defineProps<{ game: string; id: string; productType: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

// The shared search + sort, backed by the URL `?q=`/`?sort=` (survives opening a card + Back).
// The sort clamps to the known option values, falling back to the natural-order default; it's
// threaded into every section so they re-order together (the manifest is sort-independent).
const { searchInput, query, sort } = useProductCardsSearch(
  PRODUCT_CARDS_DEFAULT_SORT,
  PRODUCT_CARDS_SORT_OPTIONS.map((option) => option.value),
)
const searching = computed(() => query.value.length > 0)

// The manifest is filtered by the committed `query`, so it lists exactly the sections that
// still have matches (with recomputed counts).
const sectionsQuery = useProductCardSectionsQuery(game, id, query)
const manifest = computed(() => sectionsQuery.data.value?.data ?? [])
// A malformed search comes back as 422; surface its message and skip the (also-failing) blocks.
const searchError = computed(() => searchErrorMessage(sectionsQuery.error.value))

// The grand total across sections backs the "Cards in this product (N)" heading — the
// filtered count while a search is active.
const total = computed(() => manifest.value.reduce((sum, section) => sum + section.total, 0))

// Show the whole section (heading + search box + blocks) whenever the product has cards —
// or a search is active, so a query that currently matches nothing keeps the box on screen
// (rather than hiding it and stranding the user with no way to clear the filter).
const showSection = computed(() => searching.value || total.value > 0)

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
// key + labels + the shared search are threaded down; the manifest counts feed the grand total.
const sections = computed(() =>
  manifest.value.map((section) => ({
    key: section.key as ProductCardSectionKey,
    ...sectionMeta(section.key),
  })),
)
</script>

<template>
  <section v-if="showSection" class="mt-10">
    <h2 class="mb-4 text-sm font-semibold">
      Cards in this product
      <span class="text-muted-foreground font-normal"> ({{ total.toLocaleString() }})</span>
    </h2>

    <!-- Filter the pool with the catalog's Scryfall-style grammar (issue #222), the same
         point-and-click filter helper the catalog browse uses, plus shared size + sort. -->
    <div class="mb-6 space-y-3">
      <div class="flex max-w-xl items-center gap-2">
        <CardSearchBox
          v-model="searchInput"
          placeholder="Filter cards — name, c:r, t:goblin…"
          aria-label="Filter cards in this product"
          class="flex-1"
        />
        <AdvancedSearchPanel v-model="searchInput" />
      </div>
      <SearchSyntaxHint />
      <!-- Size + sort apply to every section (a search that matches nothing hides them). -->
      <div v-if="sections.length" class="flex flex-wrap gap-2">
        <CardSizeMenu />
        <CardSortMenu v-model="sort" :options="PRODUCT_CARDS_SORT_OPTIONS" />
      </div>
    </div>

    <p v-if="searchError" class="text-destructive text-sm">{{ searchError }}</p>
    <p v-else-if="searching && !sections.length" class="text-muted-foreground text-sm">
      No cards match “{{ query }}”.
    </p>
    <div v-else class="space-y-8">
      <ProductCardsSection
        v-for="section in sections"
        :key="section.key"
        :game="game"
        :id="id"
        :section-key="section.key"
        :title="section.title"
        :blurb="section.blurb"
        :search="query"
        :sort="sort"
      />
    </div>
  </section>
</template>
