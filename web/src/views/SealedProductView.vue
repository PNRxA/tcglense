<script setup lang="ts">
import { computed, toRef } from 'vue'
import { ArrowLeft } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import ProductDetailContent from '@/components/products/ProductDetailContent.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import { useProductQuery, useProductContentsQuery } from '@/composables/useProducts'
import { useProductBackLink } from '@/composables/useProductBackLink'
import { productImageUrl } from '@/lib/api'
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

// The full sealed-product detail page. The detail body itself lives in ProductDetailContent
// (shared with the browse-grid modal, ProductDetailDialog); this view adds what only a real
// page needs — per-URL meta/JSON-LD, breadcrumbs, and the in-app back link.
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

// Shares ProductDetailContent's ['product', game, id] key, so the page and an overlay never
// double-fetch. This view reads it only for meta/JSON-LD and page chrome.
const productQuery = useProductQuery(game, id)
const product = computed(() => productQuery.data.value)

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
  <div class="mx-auto max-w-6xl px-4 py-10">
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

    <ProductDetailContent :game="game" :id="id" />
  </div>
</template>
