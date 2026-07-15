<script setup lang="ts">
import { computed, toRef } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { useWishlistProductsQuery, WISHLIST_PRODUCT_PAGE_SIZE } from '@/composables/useWishlist'
import { useClampPage } from '@/composables/useClampPage'
import ProductGrid from '@/components/products/ProductGrid.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import { buttonVariants } from '@/components/ui/button'
import type { OwnedCountsMap } from '@/lib/api'

// The wish-list landing's sealed-products section (issue #364): a paged grid of the sealed
// products the signed-in user wants, each tile carrying the interactive quick-add control —
// its resting count comes from this page's data (`wantedById`), so no extra batch call is
// needed here. Renders nothing until data arrives and the user wants at least one product —
// discovery is carried by the quick-add box above it and the product pages, so an empty wish
// list shows no bare heading. Zeroing a want drops the tile on the settled refetch, and
// `useClampPage` handles the page shrink. Hidden entirely for signed-out users because the
// parent mounts it only in the authed branch.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')
const route = useRoute()
const router = useRouter()

// Keep this independently-paged section in the URL so opening a product and following
// its history-aware back link restores the same wish-list page (issue #414). Page 1 is
// canonicalized to an absent query key, matching the catalog list controls.
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
const query = useWishlistProductsQuery(game, page)
const entries = computed(() => query.data.value?.data ?? [])
const total = computed(() => query.data.value?.total ?? 0)
const products = computed(() => entries.value.map((e) => e.product))
// Resting counts for the tiles' quick-add controls, keyed by external product id — taken
// straight from the page data (no batch call; auth is guaranteed by the parent's authed
// branch).
const wantedById = computed<OwnedCountsMap>(() =>
  Object.fromEntries(
    entries.value.map((e) => [
      e.product.id,
      { quantity: e.quantity, foil_quantity: e.foil_quantity },
    ]),
  ),
)

useClampPage(page, () => ({
  ready: query.isSuccess.value,
  total: total.value,
  pageSize: WISHLIST_PRODUCT_PAGE_SIZE,
}))
</script>

<template>
  <section v-if="total > 0">
    <div class="mb-4 flex items-center justify-between gap-2">
      <h2 class="text-lg font-semibold">
        Sealed products
        <span class="text-muted-foreground ml-1 text-sm font-normal">{{ total }} wanted</span>
      </h2>
      <RouterLink
        :to="`/sealed/${game}`"
        :class="buttonVariants({ variant: 'outline', size: 'sm' })"
      >
        Browse sealed
      </RouterLink>
    </div>

    <ProductGrid :game="game" :products="products" :wanted="wantedById" />

    <CardPagination
      v-if="total > WISHLIST_PRODUCT_PAGE_SIZE"
      v-model:page="page"
      :page-size="WISHLIST_PRODUCT_PAGE_SIZE"
      :total="total"
      :loading="query.isPlaceholderData.value"
      class="mt-6"
    />
  </section>
</template>
