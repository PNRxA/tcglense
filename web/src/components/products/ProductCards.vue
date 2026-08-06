<script setup lang="ts">
import { computed, toRef } from 'vue'
import type { ProductCardSection, ProductCardSectionKey } from '@/lib/api'
import { useProductCardSectionsQuery } from '@/composables/useProducts'
import {
  useProductCardsSearch,
  type ProductCardsSearchKeys,
} from '@/composables/useProductCardsSearch'
import { searchErrorMessage } from '@/composables/useCardSearch'
import { boosterFamilyLabel } from '@/lib/productType'
import { productCardCounts, productCardsHeading } from '@/lib/productCounts'
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
const props = defineProps<{
  game: string
  id: string
  productType: string
  // The URL keys the search + sort ride. Left unset by the full product page, which owns its
  // route's plain `?q=`/`?sort=`; the detail modal passes namespaced keys, because the browse
  // route it overlays owns those already (see PRODUCT_CARDS_MODAL_SEARCH_KEYS). Fixed per
  // mount — a list never changes which route it renders over.
  searchKeys?: ProductCardsSearchKeys
}>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

// The shared search + sort, backed by the URL (survives opening a card + Back). The sort clamps
// to the known option values, falling back to the natural-order default; it's threaded into
// every section so they re-order together (the manifest is sort-independent). The id tells the
// composable when the list is showing a different product (a step in the modal never changes
// the path, so the id is the only signal common to both surfaces — issue #448).
const { searchInput, query, sort } = useProductCardsSearch(
  id,
  PRODUCT_CARDS_DEFAULT_SORT,
  PRODUCT_CARDS_SORT_OPTIONS.map((option) => option.value),
  props.searchKeys,
)
const searching = computed(() => query.value.length > 0)

// The manifest is filtered by the committed `query`, so it lists exactly the sections that
// still have matches (with recomputed counts).
const sectionsQuery = useProductCardSectionsQuery(game, id, query)
const manifest = computed(() => sectionsQuery.data.value?.data ?? [])
// A malformed search comes back as 422; surface its message and skip the (also-failing) blocks.
const searchError = computed(() => searchErrorMessage(sectionsQuery.error.value))

// Per-certainty counts behind the heading — guaranteed (`contains`), the booster pull pool
// (`exclusive` + `booster`), the randomized maybes (`variable`). Filtered while a search is
// active, so the heading always describes what's on screen: a search matching only booster
// cards on a bundle correctly narrows it to "What you can pull".
const counts = computed(() => productCardCounts(manifest.value))
const total = computed(() => counts.value.total)
// Heading noun + count + (mixed products only) a reconciling line. None of these numbers is a
// count of physical cards — a booster's pool is not its pack size; see lib/productCounts.ts.
// `searching` flips the instant the URL changes, but the manifest is `keepPreviousData` — so on
// *clearing* a filter the counts are still the filtered ones for a whole refetch. Treating that
// window as filtered too is what stops "(3-card pool)" being asserted over a 600-card pool;
// `isPlaceholderData` is the same signal ProductCardsSection already pages on.
const heading = computed(() =>
  productCardsHeading(counts.value, searching.value || sectionsQuery.isPlaceholderData.value),
)

// Show the whole section (heading + search box + blocks) whenever the product has cards —
// or a search is active, so a query that currently matches nothing keeps the box on screen
// (rather than hiding it and stranding the user with no way to clear the filter).
const showSection = computed(() => searching.value || total.value > 0)

// This product's *own* booster-family label ("Collector Booster", …), or null for a
// non-booster product — a fallback for naming the exclusives section when the backend
// doesn't hand one down.
const familyLabel = computed(() => boosterFamilyLabel(props.productType))

// Heading + one-line note on how strong the "is in this product" claim is, per section. The
// note shows even while collapsed (CollapsibleSection renders it in the header), which is why
// the h2 above carries a line of its own only for the mixed case.
// `hasExclusive` says the family-exclusive cards were split into their own block above, which
// makes the `booster` block the *rest* of the pool rather than all of it — it must not then
// call itself the whole pool, or the page states two sizes for one named pool.
function sectionMeta(
  section: ProductCardSection,
  hasExclusive: boolean,
): { title: string; blurb: string } {
  switch (section.key) {
    case 'contains':
      // Not "In the box" — that collides with ProductContents' "What's in the box", which
      // counts components rather than cards. The count is distinct cards (the API stores no
      // quantity), so a 100-card precon reports its ~71 different cards; say so.
      return {
        title: 'Guaranteed cards',
        blurb: 'In every copy — different cards, so extra copies of one count once.',
      }
    case 'exclusive': {
      // The exclusive section is named after its booster family. The backend hands down the
      // *contained* booster's family (a bundle wraps a collector / special booster its own
      // product_type can't express); fall back to this product's own family, then a generic
      // label.
      const family =
        (section.booster_family ? boosterFamilyLabel(section.booster_family) : null) ??
        familyLabel.value
      return {
        // Never "this booster" — the viewed product may be a bundle, which is not one.
        title: family ? `Exclusive to ${family}` : "Exclusive to this product's boosters",
        blurb: "Cards you can only pull from this product — not the set's other boosters.",
      }
    }
    case 'booster':
      // The pool is named after *this* product's family — never the exclusive section's
      // `booster_family`, which names only one. When the exclusives were split out above, this
      // block holds the pool's shared remainder, so it says "shared" and drops the claim to
      // wholeness: the family's pool is this block plus that one.
      if (hasExclusive) {
        return {
          title: familyLabel.value ? `Shared ${familyLabel.value} pool` : 'Shared booster pool',
          blurb: "The rest of the pool — cards the set's other boosters can open too.",
        }
      }
      return {
        title: familyLabel.value ? `${familyLabel.value} pull pool` : 'Booster pull pool',
        blurb:
          "Every card you can open from these boosters — the whole pool, not one pack's worth.",
      }
    case 'variable':
      return {
        title: 'May be included',
        blurb: 'Cards this product sometimes includes (a randomized configuration).',
      }
    default:
      // An unrecognised key: mirror the server, which files an unknown membership into the
      // weakest bucket rather than claiming containment.
      return { title: 'Possible cards', blurb: 'Cards this product may include.' }
  }
}

// The sections to render, in the manifest's (display) order, each dressed with its heading.
// Each block owns its own paged query (and thus its own pagination), so only the key +
// labels + the shared search are threaded down — plus the manifest count, which labels the
// block's collapsed-by-default header (issue #291) and feeds the grand total.
const sections = computed(() => {
  const hasExclusive = manifest.value.some((section) => section.key === 'exclusive')
  return manifest.value.map((section) => ({
    key: section.key as ProductCardSectionKey,
    count: section.total,
    ...sectionMeta(section, hasExclusive),
  }))
})
</script>

<template>
  <section v-if="showSection">
    <!-- The heading is worded by the certainties the manifest actually holds: a booster's
         pull pool must not be announced as cards the product contains (the pool is ~600, the
         pack holds ~15). lib/productCounts.ts owns the rule. -->
    <h2 class="text-base font-semibold tracking-tight" :class="heading.blurb ? 'mb-1' : 'mb-4'">
      {{ heading.title }}
      <span class="text-muted-foreground font-normal"> {{ heading.count }}</span>
    </h2>
    <!-- Only a mixed product (guaranteed *and* random) gets a line here: its single number
         spans two certainties, which no section blurb below can reconcile. -->
    <p v-if="heading.blurb" class="text-muted-foreground mb-4 text-xs">{{ heading.blurb }}</p>

    <!-- Filter the pool with the catalog's Scryfall-style grammar (issue #222), the same
         point-and-click filter helper the catalog browse uses, plus shared size + sort. -->
    <div class="mb-6 space-y-3">
      <div class="flex max-w-xl items-center gap-2">
        <CardSearchBox
          v-model="searchInput"
          placeholder="Filter cards — name, c:r, t:goblin…"
          aria-label="Filter this product's card list"
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
    <div v-else class="space-y-3">
      <!-- The first (most relevant) section starts expanded so the product's cards are
        visible without a click — one page fetched up front; the rest keep #291's
        collapsed-by-default economy. -->
      <ProductCardsSection
        v-for="(section, index) in sections"
        :key="section.key"
        :game="game"
        :id="id"
        :section-key="section.key"
        :title="section.title"
        :blurb="section.blurb"
        :count="section.count"
        :search="query"
        :sort="sort"
        :default-expanded="index === 0"
      />
    </div>
  </section>
</template>
