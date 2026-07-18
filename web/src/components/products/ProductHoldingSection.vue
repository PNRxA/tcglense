<script setup lang="ts">
import { computed, toRef } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import ProductGrid from '@/components/products/ProductGrid.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import { buttonVariants } from '@/components/ui/button'
import {
  useCollectionProductCounts,
  useCollectionProductsBySetQuery,
  useCollectionProductSummaryQuery,
} from '@/composables/useCollection'
import {
  useWishlistProductCounts,
  useWishlistProductsBySetQuery,
  useWishlistProductSummaryQuery,
} from '@/composables/useWishlist'
import { PRODUCT_HOLDING_SET_PAGE_SIZE } from '@/composables/productHoldingQueries'
import { useClampPage } from '@/composables/useClampPage'
import { useCurrency } from '@/composables/useCurrency'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'
import type { OwnedCountsMap, ProductHoldingSetGroup } from '@/lib/api'

const props = defineProps<{ game: string; list: CardListTarget }>()
const game = toRef(props, 'game')
const route = useRoute()
const router = useRouter()
const money = useCurrency()

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

// The body is the by-set view: one block per set, paginated by SET group (`total` counts
// sets, not products). Groups arrive newest-set-first and products are name-sorted within.
const query =
  props.list === 'wishlist'
    ? useWishlistProductsBySetQuery(game, page)
    : useCollectionProductsBySetQuery(game, page)
const groups = computed(() => query.data.value?.data ?? [])
const total = computed(() => query.data.value?.total ?? 0)

// The header count is the unique-product tally, which no longer equals the page total (sets),
// so it comes from the surface's product summary. The landing already mounts this query, so
// vue-query dedupes; while it's pending the count span self-hides.
const summaryQuery =
  props.list === 'wishlist'
    ? useWishlistProductSummaryQuery(game)
    : useCollectionProductSummaryQuery(game)
const summary = computed(() => summaryQuery.data.value)

// Count maps span every product on the page (flattened across groups): the current surface's
// counts are embedded in each entry, and the other surface is batched over the same flat list
// so its combined resting badge is authoritative too. Both maps are passed to every group's
// grid — a per-id lookup makes the extra (other-group) keys harmless.
const entries = computed(() => groups.value.flatMap((group) => group.products))
const products = computed(() => entries.value.map((entry) => entry.product))
const localCounts = computed<OwnedCountsMap>(() =>
  Object.fromEntries(
    entries.value.map((entry) => [
      entry.product.id,
      { quantity: entry.quantity, foil_quantity: entry.foil_quantity },
    ]),
  ),
)
const otherCounts =
  props.list === 'wishlist'
    ? useCollectionProductCounts(game, products).ownership
    : useWishlistProductCounts(game, products).ownership
const owned = computed(() => (props.list === 'collection' ? localCounts.value : otherCounts.value))
const wanted = computed(() => (props.list === 'wishlist' ? localCounts.value : otherCounts.value))
const countNoun = computed(() => (props.list === 'wishlist' ? 'wanted' : 'owned'))

// ProductGrid takes the bare Product payloads; each group's grid renders just its own set's.
const productsOf = (group: ProductHoldingSetGroup) => group.products.map((entry) => entry.product)

useClampPage(page, () => ({
  ready: query.isSuccess.value,
  total: total.value,
  pageSize: PRODUCT_HOLDING_SET_PAGE_SIZE,
}))
</script>

<template>
  <section v-if="total > 0">
    <div class="mb-4 flex items-center justify-between gap-2">
      <h2 class="text-lg font-semibold">
        Sealed products
        <span v-if="summary" class="text-muted-foreground ml-1 text-sm font-normal">
          {{ summary.unique_products }} {{ countNoun }}
        </span>
      </h2>
      <RouterLink
        :to="`/sealed/${game}`"
        :class="buttonVariants({ variant: 'outline', size: 'sm' })"
      >
        Browse sealed
      </RouterLink>
    </div>

    <!-- One block per set group (server order = newest set first). The set heading links to
         the sealed catalog pre-filtered to that set; the grid reuses the same count maps. -->
    <div class="space-y-8">
      <div v-for="group in groups" :key="group.code">
        <div class="mb-3 flex items-baseline gap-2">
          <RouterLink
            :to="`/sealed/${game}?set=${group.code}`"
            class="text-lg font-semibold tracking-tight hover:underline"
          >
            {{ group.name ?? group.code.toUpperCase() }}
          </RouterLink>
          <span class="text-muted-foreground text-sm">
            {{ group.unique_products }} {{ group.unique_products === 1 ? 'product' : 'products' }}
          </span>
          <span v-if="money.formatUsd(group.total_value_usd)" class="text-muted-foreground text-sm">
            · {{ money.formatUsd(group.total_value_usd) }}
          </span>
        </div>
        <ProductGrid :game="game" :products="productsOf(group)" :owned="owned" :wanted="wanted" />
      </div>
    </div>

    <CardPagination
      v-if="total > PRODUCT_HOLDING_SET_PAGE_SIZE"
      v-model:page="page"
      :page-size="PRODUCT_HOLDING_SET_PAGE_SIZE"
      :total="total"
      :loading="query.isPlaceholderData.value"
      class="mt-6"
    />
  </section>
</template>
