<script setup lang="ts">
import { computed, ref, toRef, type ComponentPublicInstance } from 'vue'
import { RouterLink } from 'vue-router'
import ProductImage from '@/components/products/ProductImage.vue'
import ProductBuyLinks from '@/components/products/ProductBuyLinks.vue'
import ProductContents from '@/components/products/ProductContents.vue'
import ProductContainers from '@/components/products/ProductContainers.vue'
import ProductCards from '@/components/products/ProductCards.vue'
import ProductOverview from '@/components/products/ProductOverview.vue'
import ProductWishlistControls from '@/components/products/ProductWishlistControls.vue'
import SetPriceAlertButton from '@/components/alerts/SetPriceAlertButton.vue'
import PriceChart from '@/components/cards/PriceChart.vue'
import PriceStatGrid from '@/components/shared/PriceStatGrid.vue'
import { Skeleton } from '@/components/ui/skeleton'
import { useProductQuery } from '@/composables/useProducts'
import type { ProductCardsSearchKeys } from '@/composables/useProductCardsSearch'
import { useCurrency } from '@/composables/useCurrency'
import { getProductPrices, type AlertFinish } from '@/lib/api'
import { productTypeLabel } from '@/lib/productType'
import { formatReleaseLabel } from '@/lib/releaseDate'

// The body of a sealed product's detail — image, prices, collection/wish-list controls,
// composition, price history, buy links, and contained cards — shared verbatim by the full
// page (SealedProductView) and the browse-grid modal (ProductDetailDialog). Page chrome
// (meta tags, breadcrumb/back link, and the modal frame) stays with the callers; both fetch
// the same ['product', game, id] key, so the page and an overlay never double-fetch.
//
// Layout: a full-width header (type, name, set + release chips) over a two-column body — a
// left rail holding the image plus everything price/ownership shaped (price tiles,
// collection + wish-list steppers, buy links), and a main column leading with the
// at-a-glance overview strip whose chips jump to the composition / cards / containers
// sections below it. The rail itself is vertical on phones and md+, but splits
// image | price-stack across the ~640-768px band where a full-width image would
// otherwise fill the screen (#573) — same shape as the card page's rail.
const props = defineProps<{
  game: string
  id: string
  // Forwarded to the contained-cards list, the one part of this body with URL-backed state of
  // its own: the modal renders over a route that already owns `?q=`/`?sort=` and so must pass
  // namespaced keys, while the page leaves this unset and keeps the plain ones.
  searchKeys?: ProductCardsSearchKeys
}>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')
const money = useCurrency()

const productQuery = useProductQuery(game, id)
const product = computed(() => productQuery.data.value)

// "Not found" once the fetch has settled without a product — not merely on `isError`: a
// 2xx with an empty body resolves to `undefined` data with `isError` false, which would
// otherwise sit on the loading skeleton forever. `!isPending` = settled, so a pending
// cache-miss still shows the skeleton below.
const notFound = computed(
  () => productQuery.isError.value || (!product.value && !productQuery.isPending.value),
)

const typeLabel = computed(() =>
  product.value ? productTypeLabel(product.value.product_type) : '',
)
const setName = computed(
  () => product.value?.set_name ?? product.value?.set_code.toUpperCase() ?? '',
)

// Current market prices + MSRP, formatted thousands-grouped; blank fields are dropped, so
// MSRP (a curated retail price, absent for most products) only appears when known.
const priceRows = computed(() => {
  const prod = product.value
  if (!prod) return []
  return [
    { label: money.displayCurrency.value, value: money.formatUsd(prod.prices?.usd) },
    {
      label: `${money.displayCurrency.value} foil`,
      value: money.formatUsd(prod.prices?.usd_foil),
    },
    { label: 'MSRP', value: money.formatUsd(prod.msrp) },
  ].filter((row): row is { label: string; value: string } => row.value != null)
})

// A future release date reads as "Releases …", a past one as "Released …", so an
// as-yet-unreleased product shows when it's due rather than claiming it already came out.
const releaseLabel = computed(() => formatReleaseLabel(product.value?.released_at))

// Sealed products are finish-less (TCGCSV is effectively single-price, and the price chart is
// single-series), so the alert dialog shows no finish picker — it watches the one available
// price. Prefer the regular column; use foil only when it's the sole priced one.
const alertFinishes = computed<AlertFinish[]>(() => {
  const prices = product.value?.prices
  if (prices?.usd == null && prices?.usd_foil != null) return ['foil']
  return ['nonfoil']
})

// Jump targets for the overview strip's chips. Template refs (not element ids) so the
// same body can render twice at once — the full page under an open detail modal —
// without colliding anchors; scrollIntoView scrolls the nearest scrollable ancestor,
// which inside the modal is the dialog body rather than the document. The refs sit on
// the section components directly (each self-hides when empty, so `$el` may be a
// placeholder comment node — hence the HTMLElement guard), keeping the main column free
// of always-rendered wrapper divs that would leave stray gaps in its spacing.
const contentsEl = ref<ComponentPublicInstance | null>(null)
const containersEl = ref<ComponentPublicInstance | null>(null)
const cardsEl = ref<ComponentPublicInstance | null>(null)
function jumpTo(target: 'contents' | 'cards' | 'containers') {
  const el = { contents: contentsEl, cards: cardsEl, containers: containersEl }[target].value?.$el
  if (el instanceof HTMLElement) el.scrollIntoView({ behavior: 'smooth', block: 'start' })
}
</script>

<template>
  <p v-if="notFound" class="text-destructive py-12">Product not found.</p>

  <template v-else>
    <!-- Header: type, name, and the set / release chips. A Skeleton stands in on a
      cache-miss deep link until the query resolves; the overview, chart + card sections
      below mount off the route params immediately, so they fetch in parallel. -->
    <header v-if="product">
      <p class="text-muted-foreground text-xs font-semibold tracking-wide uppercase">
        {{ typeLabel }}
      </p>
      <h1 class="mt-1 text-3xl font-semibold tracking-tight text-balance">{{ product.name }}</h1>
      <div class="mt-3 flex flex-wrap items-center gap-1.5 text-xs font-medium">
        <RouterLink
          :to="`/cards/${game}/sets/${product.set_code}`"
          class="bg-muted/50 hover:bg-muted inline-flex items-center gap-1.5 rounded-md border px-2 py-1 transition-colors"
        >
          {{ setName }}
          <span class="text-muted-foreground">{{ product.set_code.toUpperCase() }}</span>
        </RouterLink>
        <span v-if="releaseLabel" class="text-muted-foreground px-1">
          {{ releaseLabel.label }}
        </span>
      </div>
    </header>
    <div v-else class="space-y-3">
      <Skeleton class="h-4 w-24" />
      <Skeleton class="h-9 w-3/4" />
      <Skeleton class="h-6 w-64" />
    </div>

    <!-- Rows pinned to [auto,1fr]: row 1 hugs the rail's content and row 2 (the buy links)
      absorbs the spanning main column's surplus height — auto rows would instead split
      that surplus into row 1, opening a gap between the rail and the buy links. -->
    <div
      class="mt-8 grid items-start gap-8 md:grid-cols-[minmax(0,17rem)_1fr] md:grid-rows-[auto_1fr] md:gap-y-4 lg:grid-cols-[minmax(0,20rem)_1fr]"
    >
      <!-- Left rail: the image plus everything price/ownership shaped. Below md the rail
        is one full-width band, so an uncapped square image grew with the viewport — on
        ~640-768px (an unfolded foldable, a small tablet) that put a box swallowing the
        screen ahead of every piece of content (issue #573). In that band the rail turns
        side-by-side instead: the image keeps the rail's own 18rem and the price/ownership
        stack fills the width beside it. Phones stay stacked, md+ returns to the vertical
        rail. Mirrors the card page's rail (CardDetailContent). -->
      <aside class="flex flex-col gap-4 sm:flex-row md:col-start-1 md:row-start-1 md:flex-col">
        <template v-if="product">
          <div class="shrink-0 sm:w-72 md:w-auto">
            <ProductImage
              :game="game"
              :id="product.id"
              :name="product.name"
              :has-image="product.has_image"
              class="w-full"
            />
          </div>

          <!-- Prices, watch, and the ownership steppers — the image's neighbour in the
            sm band, its stack-mate everywhere else. -->
          <div class="min-w-0 flex-1 space-y-4">
            <!-- Current prices -->
            <div>
              <h2 class="mb-2 text-sm font-semibold">Prices</h2>
              <PriceStatGrid v-if="priceRows.length" :rows="priceRows" />
              <p v-else class="text-muted-foreground text-sm">No current price.</p>
            </div>

            <!-- Watch this product's price (issue #525). In the shared body so it shows on
                 both the full page and the browse-grid modal; shown to everyone (the dialog
                 nudges signed-out visitors to make an account). -->
            <SetPriceAlertButton
              :game="game"
              target-kind="product"
              :external-id="product.id"
              :name="product.name"
              :finishes="alertFinishes"
            />

            <!-- Independent collection + wish-list sealed holdings (#364/#435). Both gate
                 internally on auth (matching the card page), so signed-out visitors see the
                 sign-in nudges rather than a missing section. -->
            <ProductWishlistControls :game="game" :product="product" list="collection" />
            <ProductWishlistControls :game="game" :product="product" list="wishlist" />
          </div>
        </template>
        <template v-else>
          <!-- Mirrors the loaded layout, so the rail doesn't reflow when the query lands. -->
          <div class="shrink-0 sm:w-72 md:w-auto">
            <Skeleton class="aspect-square w-full rounded-lg" />
          </div>
          <div class="min-w-0 flex-1 space-y-4">
            <Skeleton class="h-24 w-full" />
            <Skeleton class="h-28 w-full" />
          </div>
        </template>
      </aside>

      <!-- Main column: overview, composition, price history, and the contained cards.
        Spans both rail rows on md+, so the buy links slot under the rail beside it. -->
      <div class="min-w-0 space-y-6 md:col-start-2 md:row-span-2 md:row-start-1">
        <!-- At-a-glance counts, each chip jumping to its section below. Mounts off the
          route id (its queries are shared with the sections), so it fills in while the
          product itself is still loading. -->
        <ProductOverview :game="game" :id="id" @jump="jumpTo" />

        <!-- What's in the box: the structural composition (nested packs/boxes linked to their
          own pages, decks, promos, physical extras). Mounts off the route id and self-hides
          when the product has no ingested composition. -->
        <ProductContents ref="contentsEl" class="scroll-mt-6" :game="game" :id="id" />

        <!-- The reverse structural relation: boxes, bundles, and other parent products that
          directly contain this product. Most useful on individual booster-pack pages; it
          self-hides for products with no parent composition (issue #415). -->
        <ProductContainers ref="containersEl" class="scroll-mt-6" :game="game" :id="id" />

        <!-- Price history over time. Keyed off game/id, so it mounts and fetches in
          parallel with the product query above. `game` overlays set-release markers.
          Sealed products are sold at a single (regular) price, so plot one line only —
          no empty foil series. -->
        <PriceChart
          :query-key="['product-prices', game, id]"
          :fetcher="(range) => getProductPrices(game, id, range)"
          :game="game"
          single-series
        />

        <!-- The cards this product contains / can be pulled from — the reverse of the
          card page's "Sealed products" section, guaranteed cards first, then this booster
          family's exclusive cards ahead of the shared pool (issue #204). Mounts off the
          route id; the family label fills in once the product loads. The first section
          starts expanded, so the most relevant cards are visible without a click. -->
        <ProductCards
          ref="cardsEl"
          class="scroll-mt-6"
          :game="game"
          :id="id"
          :product-type="product?.product_type ?? ''"
          :search-keys="searchKeys"
        />
      </div>

      <!-- Outbound "where to buy" links, grouped by region (issue #175). The rail's second
        row on md+ (right under the price/ownership stack) but LAST in source order, so the
        long store list doesn't push the product's actual content down on mobile. -->
      <ProductBuyLinks
        v-if="product"
        class="md:col-start-1 md:row-start-2"
        :game="game"
        :product="product"
      />
    </div>
  </template>
</template>
