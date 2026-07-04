<script setup lang="ts">
import { computed } from 'vue'
import BuyLinks from '@/components/cards/BuyLinks.vue'
import type { Card as CardModel } from '@/lib/api'
import { buyLinksFor } from '@/lib/buyLinks'

// "Where to buy" — outbound card-name search links per store, grouped by region
// (issue #175). No per-store prices are shown (we don't ingest them); the
// buttons just land the user on each store's results for this card. Renders
// nothing for a game with no store registry.
const props = defineProps<{ game: string; card: CardModel }>()

const sections = computed(() => buyLinksFor(props.game, props.card))
</script>

<template>
  <BuyLinks :sections="sections" />
</template>
