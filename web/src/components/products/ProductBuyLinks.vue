<script setup lang="ts">
import { computed } from 'vue'
import BuyLinks from '@/components/cards/BuyLinks.vue'
import type { Product } from '@/lib/api'
import { productBuyLinksFor } from '@/lib/buyLinks'

// "Where to buy" — outbound product-name search links per store for a sealed
// product, grouped by region (US / Australia). Mirrors CardBuyLinks over the
// product store registry: the TCGplayer entry deep-links to the exact product
// page (`product.url`) when we have it, every other store is a name search.
// Renders nothing for a game with no store registry.
const props = defineProps<{ game: string; product: Product }>()

const sections = computed(() => productBuyLinksFor(props.game, props.product))
</script>

<template>
  <BuyLinks :sections="sections" />
</template>
