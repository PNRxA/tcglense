<script setup lang="ts">
import type { Card } from '@/lib/api'
import CardTile from '@/components/cards/CardTile.vue'

defineProps<{
  game: string
  cards: Card[]
}>()
</script>

<template>
  <div
    class="grid grid-cols-2 gap-x-4 gap-y-6 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
  >
    <CardTile v-for="card in cards" :key="card.id" :game="game" :card="card" />
  </div>
</template>
