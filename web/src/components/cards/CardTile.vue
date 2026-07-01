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
const to = computed(() => `/cards/${props.game}/cards/${props.card.id}`)
</script>

<template>
  <!-- Stretched-link card: a single RouterLink (the text block) whose `after:` overlay
    covers the whole tile, so the entire card is clickable and there's exactly one link /
    tab stop whose accessible name is the card text. Crucially the #badge overlay is a
    SIBLING of that link — not nested inside the <a> — so an interactive control there
    (the quick-add popover trigger) is valid HTML and its clicks don't navigate. -->
  <div class="group relative">
    <!-- On hover the card lifts: it scales up slightly and the resting shadow deepens.
      `group-hover:z-10` raises the (already `relative`) frame above its grid neighbours so
      the enlarged card and its shadow aren't clipped by later siblings painting on top.
      The light-mode `shadow-md` is invisible on dark's near-black background, so dark mode
      gets a larger, higher-opacity shadow instead. Reduced-motion users get neither the
      grow nor the transition. -->
    <div class="relative">
      <CardImage
        :game="game"
        :id="card.id"
        :name="card.name"
        :has-image="card.has_image"
        size="normal"
        class="transition duration-200 ease-out group-hover:z-10 group-hover:scale-[1.03] group-hover:shadow-md dark:group-hover:shadow-[0_8px_24px_rgba(0,0,0,0.85)] motion-reduce:transition-none motion-reduce:group-hover:scale-100"
      />
      <!-- The image lifts to `group-hover:z-10` on hover, so overlay content must carry a
        higher z-index (the badge/quick-add control uses z-30) or the enlarged card paints
        over it. It sits above the stretched-link `after:` (z-10) too, so its buttons take
        the click instead of navigating. Browse views pass no slot, so nothing renders. -->
      <slot name="badge" />
    </div>
    <RouterLink
      :to="to"
      class="mt-1.5 block px-0.5 after:absolute after:inset-0 after:z-10 after:content-['']"
    >
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
    </RouterLink>
  </div>
</template>
