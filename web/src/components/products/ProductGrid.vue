<script setup lang="ts">
import { computed } from 'vue'
import type { OwnedCountsMap, Product } from '@/lib/api'
import ProductTile from '@/components/products/ProductTile.vue'
import ProductCountControl from '@/components/products/ProductCountControl.vue'
import { useProductNavList } from '@/composables/useProductNavList'
import { CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'
import { useAuthStore } from '@/stores/auth'

const props = defineProps<{
  game: string
  products: Product[]
  // Wanted counts keyed by external product id (from `useWishlistProductCounts`, or the
  // page data on the wish-list landing): a positive count appends a Heart chip to the
  // collection-primary quick-add badge, matching CardGrid. Omitted on signed-out grids —
  // the control only renders while signed in.
  wanted?: OwnedCountsMap
  // Owned counts for the collection twin (#435). The unified control fetches authoritative
  // collection and wish-list seeds only when its popover opens.
  owned?: OwnedCountsMap
}>()

// The grid density follows the shared card-size preference (set via CardSizeMenu) so
// the sealed grid matches the card grids the user is used to.
const cardSize = useCardSizeStore()
const gridClass = computed(() => CARD_SIZE_GRID_CLASS[cardSize.size])

// Quick-add controls (issue #364) are a signed-in feature (CardGrid parity).
const auth = useAuthStore()

// Publish the current grid order so the sealed-product modal can step with arrow keys and
// prev/next buttons, matching card-grid modal navigation.
useProductNavList(
  () => props.game,
  () => props.products.map((product) => product.id),
)
</script>

<template>
  <div class="grid gap-x-4 gap-y-6" :class="gridClass">
    <ProductTile v-for="product in products" :key="product.id" :game="game" :product="product">
      <!-- One collection-primary quick-add control, matching CardGrid: collection counts
        and the wanted Heart share one bottom-left badge, and the popover edits both lists. -->
      <template v-if="auth.isAuthenticated" #badge>
        <ProductCountControl
          :game="game"
          :product-id="product.id"
          :name="product.name"
          :quantity="owned?.[product.id]?.quantity ?? 0"
          :foil-quantity="owned?.[product.id]?.foil_quantity ?? 0"
          :wishlist-quantity="
            (wanted?.[product.id]?.quantity ?? 0) + (wanted?.[product.id]?.foil_quantity ?? 0)
          "
        />
      </template>
    </ProductTile>
  </div>
</template>
