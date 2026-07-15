<script setup lang="ts">
import { computed, toRef } from 'vue'
import { ArrowLeft } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import ProductImage from '@/components/products/ProductImage.vue'
import ProductBuyLinks from '@/components/products/ProductBuyLinks.vue'
import ProductContents from '@/components/products/ProductContents.vue'
import ProductContainers from '@/components/products/ProductContainers.vue'
import ProductCards from '@/components/products/ProductCards.vue'
import ProductWishlistControls from '@/components/products/ProductWishlistControls.vue'
import PriceChart from '@/components/cards/PriceChart.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import { Skeleton } from '@/components/ui/skeleton'
import { useProductQuery, useProductContentsQuery } from '@/composables/useProducts'
import { useProductBackLink } from '@/composables/useProductBackLink'
import { useCurrency } from '@/composables/useCurrency'
import { getProductPrices, productImageUrl } from '@/lib/api'
import { productTypeLabel } from '@/lib/productType'
import { absoluteUrl, usePageMeta } from '@/lib/seo'
import {
  breadcrumbList,
  graph,
  productMetaDescription,
  sealedCrumbs,
  sealedProductNode,
  type Crumb,
} from '@/lib/structuredData'

// The sealed-product detail page: image, name, set + type, current prices, the shared
// price-history chart, and a "Where to buy" section of outbound store links (US /
// Australia, issue #175 idiom). Mirrors CardDetailView's shape (per-URL meta/JSON-LD +
// an in-app back link), but a product has its own page only (no browse-grid modal), so
// the query + body live here directly.
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')
const money = useCurrency()

const productQuery = useProductQuery(game, id)
const product = computed(() => productQuery.data.value)
// "Not found" once the fetch has settled without a product — not merely on `isError`: a
// 2xx with an empty body resolves to `undefined` data with `isError` false, which would
// otherwise sit on the loading skeleton forever. `!isPending` = settled (no data + not
// first-loading), so a pending cache-miss still shows the skeleton below.
const notFound = computed(
  () => productQuery.isError.value || (!product.value && !productQuery.isPending.value),
)

// The product's composition ("what's in the box"), folded into the meta description + JSON-LD.
// Shares the ['product-contents', game, id] key with the <ProductContents> section below, so
// reading it here adds no extra fetch; mounts off the route refs so it loads in parallel and
// progressively upgrades the structured data once it arrives.
const contentsQuery = useProductContentsQuery(game, id)
const components = computed(() => contentsQuery.data.value?.data ?? [])

// The in-app "back" link, mirroring the page the user arrived by — a card's "Sealed
// products" section or the sealed browse — rather than always the browse (issue #203).
const backLink = useProductBackLink(game)

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

const releasedDate = computed(() => {
  const raw = product.value?.released_at
  if (!raw) return null
  const date = new Date(raw)
  return Number.isNaN(date.getTime())
    ? null
    : date.toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' })
})

// Absolute image URL for the social preview + JSON-LD; undefined when no image.
const productImage = computed(() =>
  product.value?.has_image
    ? absoluteUrl(productImageUrl(game.value, product.value.id, 'normal'))
    : undefined,
)

// Home › Sealed › {Product}, shared by the visible trail and the JSON-LD breadcrumb.
const crumbs = computed<Crumb[]>(() =>
  product.value ? sealedCrumbs(game.value, product.value) : [],
)

usePageMeta({
  title: () => product.value?.name,
  description: () =>
    product.value
      ? productMetaDescription(product.value, typeLabel.value, setName.value, components.value)
      : undefined,
  canonicalPath: () => (product.value ? `/sealed/${game.value}/${product.value.id}` : undefined),
  image: productImage,
  type: 'product',
  // A schema.org `Product` node (composition via `isRelatedTo`, deliberately NO `offers` — a
  // price tracker, not a storefront) plus a `BreadcrumbList`, in one `@graph`. Builders + the
  // no-offers rationale live in lib/structuredData.ts.
  jsonLd: () =>
    product.value
      ? graph(
          sealedProductNode(
            game.value,
            product.value,
            typeLabel.value,
            setName.value,
            components.value,
            productImage.value,
          ),
          breadcrumbList(crumbs.value),
        )
      : undefined,
})
</script>

<template>
  <div class="mx-auto max-w-5xl px-4 py-10">
    <!-- Hierarchy trail (mirrors the JSON-LD BreadcrumbList; adds a crawlable link to the
      sealed browse). The back link below stays — it's history-aware (issue #203). -->
    <PageBreadcrumbs v-if="crumbs.length" :items="crumbs" />

    <RouterLink
      :to="backLink.to"
      class="text-muted-foreground hover:text-foreground mb-6 inline-flex items-center gap-1.5 text-sm"
    >
      <ArrowLeft class="size-4" />
      {{ backLink.label }}
    </RouterLink>

    <p v-if="notFound" class="text-destructive py-12">Product not found.</p>

    <template v-else>
      <!-- Product body — image + name + prices. A Skeleton stands in on a cache-miss
        deep link until the query resolves; the chart + card sections below mount off the
        route params immediately, so they fetch in parallel rather than waiting. -->
      <div v-if="!product" class="grid gap-8 md:grid-cols-[minmax(0,20rem)_1fr]">
        <Skeleton class="aspect-square w-full rounded-lg" />
        <div class="space-y-3">
          <Skeleton class="h-4 w-24" />
          <Skeleton class="h-9 w-3/4" />
          <Skeleton class="h-4 w-40" />
          <div class="mt-6 grid grid-cols-2 gap-2">
            <Skeleton class="h-16" />
            <Skeleton class="h-16" />
          </div>
        </div>
      </div>

      <div v-else class="grid gap-8 md:grid-cols-[minmax(0,20rem)_1fr]">
        <ProductImage
          :game="game"
          :id="product.id"
          :name="product.name"
          :has-image="product.has_image"
          class="w-full"
        />

        <div>
          <p class="text-muted-foreground text-sm">{{ typeLabel }}</p>
          <h1 class="mt-1 text-3xl font-semibold tracking-tight">{{ product.name }}</h1>

          <p class="text-muted-foreground mt-2 text-sm">
            <RouterLink
              :to="`/cards/${game}/sets/${product.set_code}`"
              class="hover:text-foreground hover:underline"
            >
              {{ setName }}
            </RouterLink>
            <template v-if="releasedDate"> · Released {{ releasedDate }}</template>
          </p>

          <!-- Current prices -->
          <div v-if="priceRows.length" class="mt-6">
            <h2 class="mb-2 text-sm font-semibold">Prices</h2>
            <dl class="grid grid-cols-2 gap-2">
              <div
                v-for="row in priceRows"
                :key="row.label"
                class="bg-muted/50 rounded-lg border p-3"
              >
                <dt class="text-muted-foreground text-xs">{{ row.label }}</dt>
                <dd class="mt-0.5 font-medium tabular-nums">{{ row.value }}</dd>
              </div>
            </dl>
          </div>
          <p v-else class="text-muted-foreground mt-6 text-sm">No current price.</p>

          <!-- "Regular add" to the wish list (issue #364): read/adjust how many of this sealed
            product you want. Wishlist-only (no collection sealed surface); signed-out visitors
            get a sign-in nudge. -->
          <ProductWishlistControls :game="game" :product="product" />
        </div>
      </div>

      <!-- What's in the box: the structural composition (nested packs/boxes linked to their
        own pages, decks, promos, physical extras). Mounts off the route id and self-hides
        when the product has no ingested composition. -->
      <ProductContents :game="game" :id="id" />

      <!-- The reverse structural relation: boxes, bundles, and other parent products that
        directly contain this product. Most useful on individual booster-pack pages; it
        self-hides for products with no parent composition (issue #415). -->
      <ProductContainers :game="game" :id="id" />

      <!-- Price history over time (full width, below the details). Keyed off game/id, so
        it mounts and fetches in parallel with the product query above. -->
      <PriceChart
        :query-key="['product-prices', game, id]"
        :fetcher="(range) => getProductPrices(game, id, range)"
      />

      <!-- Outbound "where to buy" links, grouped by region (issue #175). Needs the full
        product object, so it waits for the fetch (the sections below key off game/id). -->
      <ProductBuyLinks v-if="product" :game="game" :product="product" />

      <!-- The cards this product contains / can be pulled from — the reverse of the
        card page's "Sealed products" section, guaranteed cards first, then this booster
        family's exclusive cards ahead of the shared pool (issue #204). Mounts off the
        route id; the family label fills in once the product loads. -->
      <ProductCards :game="game" :id="id" :product-type="product?.product_type ?? ''" />
    </template>
  </div>
</template>
