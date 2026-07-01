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
    <!-- On hover the card lifts: it scales up slightly and the resting shadow
      deepens. `group-hover:z-10` raises the (already `relative`) frame above its
      grid neighbours so the enlarged card and its shadow aren't clipped by later
      siblings painting on top. The light-mode `shadow-md` is invisible on dark's
      near-black background, so dark mode gets a larger, higher-opacity shadow
      instead. Reduced-motion users get neither the grow nor the transition. -->
    <!-- `relative` anchors the optional #badge overlay (e.g. an owned-count chip in
      the collection grid); browse views pass no slot, so nothing renders there.
      The image lifts to `group-hover:z-10` on hover, so badge content must carry a
      higher `z-20` or the enlarged card paints over it and the badges disappear. -->
    <div class="relative">
      <CardImage
        :game="game"
        :id="card.id"
        :name="card.name"
        :has-image="card.has_image"
        size="normal"
        class="transition duration-200 ease-out group-hover:z-10 group-hover:scale-[1.03] group-hover:shadow-md dark:group-hover:shadow-[0_8px_24px_rgba(0,0,0,0.85)] motion-reduce:transition-none motion-reduce:group-hover:scale-100"
      />
      <slot name="badge" />
    </div>
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
