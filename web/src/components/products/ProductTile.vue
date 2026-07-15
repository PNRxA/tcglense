<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import type { Product } from '@/lib/api'
import { displayUsdPrice } from '@/lib/cardPrice'
import { useCurrency } from '@/composables/useCurrency'
import { productTypeLabel } from '@/lib/productType'
import { prefetchRouteChunks } from '@/lib/prefetch'
import ProductImage from '@/components/products/ProductImage.vue'

const props = defineProps<{
  game: string
  product: Product
}>()

const money = useCurrency()

// The regular USD price, falling back to the foil price for foil-only products;
// formatted thousands-grouped (a sealed box runs to hundreds of dollars).
const price = computed(() => {
  const pick = displayUsdPrice(props.product.prices)
  return pick ? { text: money.formatUsd(pick.amount), foil: pick.foil } : null
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
      <!-- The slotted control self-positions (bottom-2 left-2 z-20) and may be interactive:
        the image lifts to `group-hover:z-10` on hover, so overlay content must carry a higher
        z-index (the quick-add control uses z-20) or the enlarged card paints over it. z-20
        sits above the stretched-link `after:` (z-10) too, so its buttons take the click
        instead of navigating — a <button> in the slot is valid HTML here because the slot is
        a SIBLING of the RouterLink, not nested inside it (the CardTile idiom). Browse grids
        pass no slot when signed out, so nothing renders. -->
      <slot name="badge" />
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
