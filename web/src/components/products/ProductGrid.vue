<script setup lang="ts">
import { computed } from 'vue'
import type { Product } from '@/lib/api'
import ProductTile from '@/components/products/ProductTile.vue'
import { CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'

defineProps<{
  game: string
  products: Product[]
}>()

// The grid density follows the shared card-size preference (set via CardSizeMenu) so
// the sealed grid matches the card grids the user is used to.
const cardSize = useCardSizeStore()
const gridClass = computed(() => CARD_SIZE_GRID_CLASS[cardSize.size])
</script>

<template>
  <div class="grid gap-x-4 gap-y-6" :class="gridClass">
    <ProductTile v-for="product in products" :key="product.id" :game="game" :product="product" />
  </div>
</template>
