<script setup lang="ts">
import { RouterLink } from 'vue-router'
import type { Card } from '@/lib/api'
import CardImage from '@/components/cards/CardImage.vue'

defineProps<{
  game: string
  card: Card
}>()
</script>

<template>
  <RouterLink :to="`/cards/${game}/cards/${card.id}`" class="group block">
    <CardImage
      :game="game"
      :id="card.id"
      :name="card.name"
      :has-image="card.has_image"
      size="normal"
      class="transition-shadow group-hover:shadow-md"
    />
    <div class="mt-1.5 px-0.5">
      <p class="truncate text-sm font-medium group-hover:underline" :title="card.name">
        {{ card.name }}
      </p>
      <p class="text-muted-foreground flex items-center justify-between gap-2 text-xs">
        <span class="truncate"
          >{{ card.set_code.toUpperCase() }} · #{{ card.collector_number }}</span
        >
        <span v-if="card.prices.usd" class="shrink-0 tabular-nums">${{ card.prices.usd }}</span>
      </p>
    </div>
  </RouterLink>
</template>
