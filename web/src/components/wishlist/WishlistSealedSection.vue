<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { RouterLink } from 'vue-router'
import { useWishlistProductsQuery, WISHLIST_PRODUCT_PAGE_SIZE } from '@/composables/useWishlist'
import { useClampPage } from '@/composables/useClampPage'
import ProductGrid from '@/components/products/ProductGrid.vue'
import OwnedCountBadge from '@/components/cards/OwnedCountBadge.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import { buttonVariants } from '@/components/ui/button'

// The wish-list landing's sealed-products section (issue #364): a paged grid of the sealed
// products the signed-in user wants, each tile carrying a static wanted-count badge.
// Renders nothing until data arrives and the user wants at least one product — discovery is
// carried by the quick-add box above it and the product pages, so an empty wish list shows
// no bare heading. Badges are static: edits happen on the product page or via quick add, not
// here. Hidden entirely for signed-out users because the parent mounts it only in the authed
// branch.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')
const page = ref(1)
const query = useWishlistProductsQuery(game, page)
const entries = computed(() => query.data.value?.data ?? [])
const total = computed(() => query.data.value?.total ?? 0)
const products = computed(() => entries.value.map((e) => e.product))
const countsById = computed(() => Object.fromEntries(entries.value.map((e) => [e.product.id, e])))

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

    <ProductGrid :game="game" :products="products">
      <template #badge="{ product }">
        <OwnedCountBadge
          :quantity="countsById[product.id]?.quantity ?? 0"
          :foil-quantity="countsById[product.id]?.foil_quantity ?? 0"
        />
      </template>
    </ProductGrid>

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
