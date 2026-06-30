<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import type { Card } from '@/lib/api'
import { displayUsdPrice } from '@/lib/cardPrice'
import CardImage from '@/components/cards/CardImage.vue'

const props = defineProps<{
  game: string
  card: Card
}>()

// Show the regular USD price, falling back to the foil price for foil-only cards.
const price = computed(() => displayUsdPrice(props.card.prices))
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
        <span v-if="price" class="shrink-0 tabular-nums"
          >${{ price.amount
          }}<span
            v-if="price.foil"
            class="ml-1 text-[0.65rem] tracking-wide uppercase opacity-70"
            title="Foil price (no regular printing)"
            >foil</span
          ></span
        >
      </p>
    </div>
  </RouterLink>
</template>
