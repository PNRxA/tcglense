<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import type { Product } from '@/lib/api'
import { displayUsdPrice } from '@/lib/cardPrice'
import { formatUsd } from '@/lib/money'
import { productTypeLabel } from '@/lib/productType'
import { prefetchRouteChunks } from '@/lib/prefetch'
import ProductImage from '@/components/products/ProductImage.vue'

const props = defineProps<{
  game: string
  product: Product
}>()

// The regular USD price, falling back to the foil price for foil-only products;
// formatted thousands-grouped (a sealed box runs to hundreds of dollars).
const price = computed(() => {
  const pick = displayUsdPrice(props.product.prices)
  return pick ? { text: formatUsd(pick.amount), foil: pick.foil } : null
})
const typeLabel = computed(() => productTypeLabel(props.product.product_type))
const to = computed(() => `/sealed/${props.game}/${props.product.id}`)

// Warm the product page's JS chunk on hover/focus so the click opens a loaded view
// (see lib/prefetch.ts — chunks only, never data/images).
const router = useRouter()
const warm = () => prefetchRouteChunks(router, to.value)
</script>

<template>
  <!-- Stretched-link card: the RouterLink's `after:` overlay covers the whole tile so
    the image + text are one clickable target and one tab stop. Unlike CardTile there's
    no detail modal — sealed products open their own detail page directly. -->
  <div class="group relative">
    <div class="relative">
      <ProductImage
        :game="game"
        :id="product.id"
        :name="product.name"
        :has-image="product.has_image"
        size="normal"
        class="transition duration-200 ease-out group-hover:z-10 group-hover:scale-[1.02] group-hover:shadow-md dark:group-hover:shadow-[0_8px_24px_rgba(0,0,0,0.85)] motion-reduce:transition-none motion-reduce:group-hover:scale-100"
      />
      <!-- Corner overlay for a per-product badge (the wish list's static wanted-count chip,
        issue #364), scoped to the image like CardTile's #badge. z-20 keeps it above the
        RouterLink's stretched `after:` overlay (z-10) and the hover-lifted image; the badges
        this feature ships are static counts, so `pointer-events-none` lets clicks fall through
        to the link — a future interactive control can lift that. Browse grids pass no slot, so
        nothing renders. -->
      <div v-if="$slots.badge" class="pointer-events-none absolute bottom-2 left-2 z-20">
        <slot name="badge" />
      </div>
    </div>
    <RouterLink
      :to="to"
      class="mt-1.5 block px-0.5 after:absolute after:inset-0 after:z-10 after:content-['']"
      @pointerenter="warm"
      @focusin="warm"
    >
      <p class="truncate text-sm font-medium group-hover:underline" :title="product.name">
        {{ product.name }}
      </p>
      <p class="text-muted-foreground truncate text-xs">
        {{ product.set_name ?? product.set_code.toUpperCase() }} · {{ typeLabel }}
      </p>
      <p class="mt-0.5 text-sm font-medium tabular-nums">
        <template v-if="price"
          >{{ price.text
          }}<span
            v-if="price.foil"
            class="text-muted-foreground ml-1 text-[0.65rem] tracking-wide uppercase"
            title="Foil price (no regular listing)"
            >foil</span
          ></template
        >
        <span v-else class="text-muted-foreground">—</span>
      </p>
    </RouterLink>
  </div>
</template>
