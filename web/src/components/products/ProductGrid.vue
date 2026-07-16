<script setup lang="ts">
import { computed } from 'vue'
import type { OwnedCountsMap, Product } from '@/lib/api'
import ProductTile from '@/components/products/ProductTile.vue'
import WantedCountControl from '@/components/products/WantedCountControl.vue'
import { CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'
import { useAuthStore } from '@/stores/auth'

defineProps<{
  game: string
  products: Product[]
  // Wanted counts keyed by external product id (from `useWishlistProductCounts`, or the
  // page data on the wish-list landing): a product present here rests as a count chip on its
  // quick-add control; an absent product (or an absent map) rests as a "+". Omitted on
  // signed-out grids — the controls only render while signed in, so a signed-out visitor's
  // grid carries neither badges nor add affordances.
  wanted?: OwnedCountsMap
  // Owned counts for the collection twin (#435). Both controls share the same component
  // and fetch their authoritative single-entry seed only when opened.
  owned?: OwnedCountsMap
}>()

// The grid density follows the shared card-size preference (set via CardSizeMenu) so
// the sealed grid matches the card grids the user is used to.
const cardSize = useCardSizeStore()
const gridClass = computed(() => CARD_SIZE_GRID_CLASS[cardSize.size])

// Quick-add controls (issue #364) are a signed-in feature (CardGrid parity).
const auth = useAuthStore()
</script>

<template>
  <div class="grid gap-x-4 gap-y-6" :class="gridClass">
    <ProductTile v-for="product in products" :key="product.id" :game="game" :product="product">
      <!-- Wish-list quick-add (issue #364 follow-up): signed-in only, like CardGrid's
        control. `wanted` supplies the resting badge counts; an absent map (or an absent
        product in it) rests that tile as a "+" — the authoritative want loads when the
        popover opens. -->
      <template v-if="auth.isAuthenticated" #badge>
        <WantedCountControl
          list="collection"
          :game="game"
          :product-id="product.id"
          :name="product.name"
          :quantity="owned?.[product.id]?.quantity ?? 0"
          :foil-quantity="owned?.[product.id]?.foil_quantity ?? 0"
        />
        <WantedCountControl
          list="wishlist"
          :game="game"
          :product-id="product.id"
          :name="product.name"
          :quantity="wanted?.[product.id]?.quantity ?? 0"
          :foil-quantity="wanted?.[product.id]?.foil_quantity ?? 0"
        />
      </template>
    </ProductTile>
  </div>
</template>
