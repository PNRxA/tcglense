<script setup lang="ts">
import { computed, toRef } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import UpdatingOverlay from '@/components/cards/UpdatingOverlay.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import ProductGrid from '@/components/products/ProductGrid.vue'
import ProductGridSkeleton from '@/components/products/ProductGridSkeleton.vue'
import CollectionSignInPrompt from '@/components/collection/CollectionSignInPrompt.vue'
import { useGameName } from '@/composables/useCatalog'
import { useClampPage } from '@/composables/useClampPage'
import { useCurrency } from '@/composables/useCurrency'
import {
  useCollectionProductCounts,
  useCollectionProductSetsQuery,
  useCollectionProductsQuery,
  useCollectionProductSummaryQuery,
} from '@/composables/useCollection'
import {
  useWishlistProductCounts,
  useWishlistProductSetsQuery,
  useWishlistProductsQuery,
  useWishlistProductSummaryQuery,
} from '@/composables/useWishlist'
import { PRODUCT_HOLDING_PAGE_SIZE } from '@/composables/productHoldingQueries'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'
import type { OwnedCountsMap } from '@/lib/api'

// The set-scoped (and all-products) sealed-holdings grid — the click-through target of the
// landing's ProductSetTile grid, the sealed mirror of CollectionBrowseView. ONE component serves
// four routes: collection/wishlist × all/set-scoped. Collection and wish list are twin surfaces,
// so the surface (`list`) only picks which query hooks to instantiate and the wording; the whole
// template is shared. `code` (undefined = the all-products view) is the only per-route difference
// beyond the surface, and there is no in-app link crossing surfaces, so `list` is fixed per mount.
const props = defineProps<{ game: string; code?: string; list: CardListTarget }>()
const game = toRef(props, 'game')
const gameName = useGameName(game)
const money = useCurrency()
const auth = useAuthStore()
const route = useRoute()
const router = useRouter()

const isWishlist = props.list === 'wishlist'
const basePath = isWishlist ? '/wishlist' : '/collection'
const rootLabel = isWishlist ? 'Wish list' : 'Collection'
const useSetsQuery = isWishlist ? useWishlistProductSetsQuery : useCollectionProductSetsQuery
const useProductsQuery = isWishlist ? useWishlistProductsQuery : useCollectionProductsQuery
const useSummaryQuery = isWishlist
  ? useWishlistProductSummaryQuery
  : useCollectionProductSummaryQuery
const useOtherCounts = isWishlist ? useCollectionProductCounts : useWishlistProductCounts

const scoped = computed(() => !!props.code)
const setCode = computed(() => props.code || undefined)

// The URL is the source of truth for the page number, so it survives opening a product and
// pressing Back and is shareable/reload-safe (mirroring the card browse views).
const page = computed({
  get: () => {
    const value = Number(route.query.page)
    return Number.isInteger(value) && value > 1 ? value : 1
  },
  set: (value) => {
    const query = { ...route.query }
    if (value > 1) query.page = String(value)
    else delete query.page
    void router.replace({ query })
  },
})

// The held-product sets resolve the scoped set's display name + header stats; the landing already
// mounts this query, so vue-query dedupes.
const setsQuery = useSetsQuery(game)
const scopedSet = computed(() =>
  scoped.value ? setsQuery.data.value?.data.find((s) => s.code === props.code) : undefined,
)
const heading = computed(() =>
  scoped.value ? (scopedSet.value?.name ?? props.code?.toUpperCase() ?? '') : 'All sealed products',
)

// Header count line: the scoped set's tally + value when set-scoped, else the whole surface's
// product summary. Both formatted through useCurrency.
const summaryQuery = useSummaryQuery(game)
const countProducts = computed(() =>
  scoped.value ? scopedSet.value?.unique_products : summaryQuery.data.value?.unique_products,
)
const countValue = computed(() =>
  money.formatUsd(
    scoped.value ? scopedSet.value?.total_value_usd : summaryQuery.data.value?.total_value_usd,
  ),
)

// The flat (optionally set-scoped) product page. `setCode` rides the query key so a set change
// refetches, and threads through to the `?set=` param.
const productsQuery = useProductsQuery(game, page, setCode)
const entries = computed(() => productsQuery.data.value?.data ?? [])
const products = computed(() => entries.value.map((entry) => entry.product))
const total = computed(() => productsQuery.data.value?.total ?? 0)

// The current surface's counts come from the page entries; the OTHER surface is batched over the
// same products so its combined resting badge is authoritative too (a per-id lookup in the grid).
const localCounts = computed<OwnedCountsMap>(() =>
  Object.fromEntries(
    entries.value.map((entry) => [
      entry.product.id,
      { quantity: entry.quantity, foil_quantity: entry.foil_quantity },
    ]),
  ),
)
const otherCounts = useOtherCounts(game, products).ownership
const owned = computed(() => (isWishlist ? otherCounts.value : localCounts.value))
const wanted = computed(() => (isWishlist ? localCounts.value : otherCounts.value))

useClampPage(page, () => ({
  ready: productsQuery.isSuccess.value,
  total: total.value,
  pageSize: PRODUCT_HOLDING_PAGE_SIZE,
}))

// Per-account page — kept out of search indexes.
const title = computed(() => {
  const noun = isWishlist ? 'wanted sealed products' : 'sealed products'
  return scoped.value
    ? `${heading.value} — your ${gameName.value} ${noun}`
    : `Your ${gameName.value} ${noun}`
})
usePageMeta({
  title: () => title.value,
  canonicalPath: () =>
    scoped.value
      ? `${basePath}/${game.value}/products/sets/${props.code}`
      : `${basePath}/${game.value}/products`,
  noindex: true,
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <PageBreadcrumbs
      :items="[
        { label: rootLabel, to: basePath },
        { label: gameName, to: `${basePath}/${game}` },
        { label: scoped ? heading : 'Sealed products' },
      ]"
    />

    <!-- Signed out (session resolved): prompt to sign in rather than bouncing to the login
         page, preserving ?redirect (matches the landing + card browse views). While the initial
         session is still resolving, show the pending grid instead of flashing the prompt. -->
    <CollectionSignInPrompt
      v-if="auth.sessionResolved && !auth.isAuthenticated"
      :game-name="gameName"
      :list="list"
    />

    <template v-else-if="auth.isAuthenticated">
      <header class="mb-6">
        <h1 class="text-3xl font-semibold tracking-tight">{{ heading }}</h1>
        <p v-if="countProducts != null" class="text-muted-foreground mt-1 text-sm tabular-nums">
          {{ countProducts.toLocaleString() }} {{ countProducts === 1 ? 'product' : 'products' }}
          <template v-if="countValue"> · {{ countValue }}</template>
        </p>
      </header>

      <ProductGridSkeleton v-if="productsQuery.isPending.value" />
      <p v-else-if="productsQuery.isError.value" class="text-destructive py-12">
        Couldn't load sealed products. Please retry.
      </p>
      <p v-else-if="!products.length" class="text-muted-foreground py-12">
        No sealed products here yet.
      </p>

      <template v-else>
        <UpdatingOverlay :loading="productsQuery.isPlaceholderData.value">
          <ProductGrid :game="game" :products="products" :owned="owned" :wanted="wanted" />
        </UpdatingOverlay>
        <CardPagination
          v-if="total > PRODUCT_HOLDING_PAGE_SIZE"
          v-model:page="page"
          :page-size="PRODUCT_HOLDING_PAGE_SIZE"
          :total="total"
          :loading="productsQuery.isPlaceholderData.value"
          class="mt-10"
        />
      </template>
    </template>

    <!-- Initial session still resolving: reserve the grid's layout. -->
    <ProductGridSkeleton v-else />
  </div>
</template>
