<script setup lang="ts">
import { computed, toRef } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import ProductGrid from '@/components/products/ProductGrid.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import { buttonVariants } from '@/components/ui/button'
import {
  COLLECTION_PRODUCT_PAGE_SIZE,
  useCollectionProductCounts,
  useCollectionProductsQuery,
} from '@/composables/useCollection'
import {
  WISHLIST_PRODUCT_PAGE_SIZE,
  useWishlistProductCounts,
  useWishlistProductsQuery,
} from '@/composables/useWishlist'
import { useClampPage } from '@/composables/useClampPage'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'
import type { OwnedCountsMap } from '@/lib/api'

const props = defineProps<{ game: string; list: CardListTarget }>()
const game = toRef(props, 'game')
const route = useRoute()
const router = useRouter()

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

const query =
  props.list === 'wishlist'
    ? useWishlistProductsQuery(game, page)
    : useCollectionProductsQuery(game, page)
const entries = computed(() => query.data.value?.data ?? [])
const total = computed(() => query.data.value?.total ?? 0)
const products = computed(() => entries.value.map((entry) => entry.product))
const localCounts = computed<OwnedCountsMap>(() =>
  Object.fromEntries(
    entries.value.map((entry) => [
      entry.product.id,
      { quantity: entry.quantity, foil_quantity: entry.foil_quantity },
    ]),
  ),
)
// ProductGrid's unified control exposes both lists. The current surface's counts are already
// embedded in the page; fetch the other surface in one batch so its combined resting badge
// is authoritative too (and never shows a misleading zero for a cross-listed product).
const otherCounts =
  props.list === 'wishlist'
    ? useCollectionProductCounts(game, products).ownership
    : useWishlistProductCounts(game, products).ownership
const owned = computed(() => (props.list === 'collection' ? localCounts.value : otherCounts.value))
const wanted = computed(() => (props.list === 'wishlist' ? localCounts.value : otherCounts.value))
const pageSize =
  props.list === 'wishlist' ? WISHLIST_PRODUCT_PAGE_SIZE : COLLECTION_PRODUCT_PAGE_SIZE
const countNoun = computed(() => (props.list === 'wishlist' ? 'wanted' : 'owned'))

useClampPage(page, () => ({
  ready: query.isSuccess.value,
  total: total.value,
  pageSize,
}))
</script>

<template>
  <section v-if="total > 0">
    <div class="mb-4 flex items-center justify-between gap-2">
      <h2 class="text-lg font-semibold">
        Sealed products
        <span class="text-muted-foreground ml-1 text-sm font-normal">
          {{ total }} {{ countNoun }}
        </span>
      </h2>
      <RouterLink
        :to="`/sealed/${game}`"
        :class="buttonVariants({ variant: 'outline', size: 'sm' })"
      >
        Browse sealed
      </RouterLink>
    </div>

    <ProductGrid :game="game" :products="products" :owned="owned" :wanted="wanted" />

    <CardPagination
      v-if="total > pageSize"
      v-model:page="page"
      :page-size="pageSize"
      :total="total"
      :loading="query.isPlaceholderData.value"
      class="mt-6"
    />
  </section>
</template>
