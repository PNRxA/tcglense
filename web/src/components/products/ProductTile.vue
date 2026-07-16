<script lang="ts">
// Warm the shared product-detail dialog chunk on the first hover/focus of any tile.
let dialogWarmed = false
</script>

<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import type { Product } from '@/lib/api'
import { displayUsdPrice } from '@/lib/cardPrice'
import { useCurrency } from '@/composables/useCurrency'
import { productTypeLabel } from '@/lib/productType'
import ProductImage from '@/components/products/ProductImage.vue'
import { loadProductDetailDialog } from '@/components/products/detailDialogLoader'

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

// Plain left-clicks open the shared modal over the current browse route. The anchor keeps
// the real product page as its href so modifiers, middle-click, new-tab, and crawlers retain
// normal navigation. Opening from a card modal swaps the query key rather than stacking two
// dialogs; CardTile performs the inverse transition for cards inside a product modal.
const route = useRoute()
const router = useRouter()
const href = computed(() => router.resolve(to.value).href)
function onClick(event: MouseEvent) {
  if (event.defaultPrevented) return
  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
    return
  }
  event.preventDefault()
  const query: LocationQueryRaw = { ...route.query, product: props.product.id }
  delete query.card
  if (typeof route.params.game !== 'string' || !route.params.game) query.game = props.game
  void router.push({ query })
}

function warmProductDetailDialog() {
  if (dialogWarmed) return
  dialogWarmed = true
  void loadProductDetailDialog()
}
</script>

<template>
  <!-- Stretched-link card: the anchor's `after:` overlay covers the whole tile so
    the image + text are one clickable target and one tab stop. A plain click opens the
    product modal; the href remains its canonical detail page. -->
  <div
    class="group relative"
    @pointerenter="warmProductDetailDialog"
    @focusin="warmProductDetailDialog"
  >
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
        a SIBLING of the anchor, not nested inside it (the CardTile idiom). Browse grids
        pass no slot when signed out, so nothing renders. -->
      <slot name="badge" />
    </div>
    <a
      :href="href"
      class="mt-1.5 block px-0.5 after:absolute after:inset-0 after:z-10 after:content-['']"
      @click="onClick"
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
    </a>
  </div>
</template>
