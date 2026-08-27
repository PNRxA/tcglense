<script lang="ts">
// Warm the shared card-detail dialog chunk on the first hover/focus of ANY row (module
// flag → once per session), mirroring CardTile, so the click that opens ?card= finds
// the chunk already fetched.
let dialogWarmed = false
</script>

<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { Layers, Sparkles } from '@lucide/vue'
import type { CollectionMover, CollectionSealedMover } from '@/lib/api'
import CardImage from '@/components/cards/CardImage.vue'
import ProductImage from '@/components/products/ProductImage.vue'
import { loadCardDetailDialog } from '@/components/cards/detailDialogLoader'
import { useCurrency } from '@/composables/useCurrency'
import { prefetchRouteChunks } from '@/lib/prefetch'
import { productTypeLabel } from '@/lib/productType'

// One card or sealed-product row in the collection's "Biggest movers" panel. Card clicks
// preserve the landing under the shared detail modal; product clicks use the sealed detail
// route. Both keep real hrefs for modifier/middle clicks and new tabs.
const props = defineProps<{ game: string; mover: CollectionMover | CollectionSealedMover }>()
const money = useCurrency()

const route = useRoute()
const router = useRouter()
const card = computed(() => ('card' in props.mover ? props.mover.card : null))
const product = computed(() => ('product' in props.mover ? props.mover.product : null))
const isProduct = computed(() => product.value != null)
const itemName = computed(() => product.value?.name ?? card.value?.name ?? 'Unknown item')

// The set/type sub-line, unified so the owned-count indicator can sit beside it for either
// item kind: cards read "SET · #123", sealed products read "Set name · Bundle".
const metaText = computed(() => {
  if (product.value) {
    const set = product.value.set_name ?? product.value.set_code.toUpperCase()
    return `${set} · ${productTypeLabel(product.value.product_type)}`
  }
  if (card.value) {
    return `${card.value.set_code.toUpperCase()} · #${card.value.collector_number}`
  }
  return ''
})

// How many of this item the user holds — context only: the movement shown is the price
// change of ONE copy (never multiplied by these counts), so the badges say how many copies
// the news applies to without changing the figures. Mirrors OwnedCountBadge's semantics — a
// total (regular + foil) with a stacked-cards icon, plus a foil count with a sparkles icon
// only when some copies are foil — but rendered in the row's muted inline style rather than
// the solid overlay pill that badge uses over card art.
const ownedTotal = computed(() => props.mover.quantity + props.mover.foil_quantity)
const foilCount = computed(() => props.mover.foil_quantity)
const to = computed(() =>
  isProduct.value && product.value
    ? `/sealed/${props.game}/${product.value.id}`
    : `/cards/${props.game}/cards/${card.value?.id ?? ''}`,
)
const href = computed(() => router.resolve(to.value).href)
function onClick(event: MouseEvent) {
  if (event.defaultPrevented) return
  // Let the browser handle anything that isn't a plain left-click.
  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
    return
  }
  event.preventDefault()
  if (isProduct.value) {
    void router.push(to.value)
  } else if (card.value) {
    void router.push({ query: { ...route.query, card: card.value.id } })
  }
}

// Fire-and-forget prefetch of the relevant detail surface on hover/focus.
function warmDetail() {
  if (isProduct.value) {
    prefetchRouteChunks(router, to.value)
  } else if (!dialogWarmed) {
    dialogWarmed = true
    void loadCardDetailDialog()
  }
}

// Gain/loss is read off the change itself (gainers are positive, losers negative).
// `change_usd` is a SIGNED decimal string, so the sign is stripped before formatUsd
// (which would otherwise render "$-3.50") and re-applied as a real minus (U+2212),
// whose glyph width matches the plus.
const change = computed(() => Number(props.mover.change_usd))
const isGain = computed(() => change.value >= 0)
const changeText = computed(() => {
  if (!Number.isFinite(change.value)) return props.mover.change_usd
  return `${isGain.value ? '+' : '−'}${money.formatUsd(String(Math.abs(change.value)))}`
})
const pctText = computed(() => {
  const pct = props.mover.change_pct
  if (pct == null) return null
  return `${pct >= 0 ? '+' : '−'}${Math.abs(pct).toFixed(1)}%`
})
// The represented finish's single-copy price today; `foil` marks the rows whose movement is
// the foil printing's, so a $25 figure over a $5 regular isn't read as a data error.
const priceNow = computed(() => money.formatUsd(props.mover.price_now))
const isFoilPrice = computed(() => props.mover.foil)

// The success/destructive tokens keep the 4.5:1 WCAG AA contrast threshold on the
// white card, matching the chips.
const deltaClass = computed(() => (isGain.value ? 'text-success' : 'text-destructive'))
const chipClass = computed(() =>
  isGain.value ? 'bg-success/15 text-success' : 'bg-destructive/15 text-destructive',
)
</script>

<template>
  <a
    :href="href"
    class="group hover:bg-muted/50 -mx-2 flex items-center gap-3 rounded-md px-2 py-1.5"
    @click="onClick"
    @pointerenter="warmDetail"
    @focusin="warmDetail"
  >
    <CardImage
      v-if="card"
      :game="game"
      :id="card.id"
      :name="card.name"
      :has-image="card.has_image"
      size="small"
      class="w-10 shrink-0"
    />
    <ProductImage
      v-else-if="product"
      :game="game"
      :id="product.id"
      :name="product.name"
      :has-image="product.has_image"
      size="small"
      class="w-10 shrink-0"
    />
    <div class="min-w-0 flex-1">
      <p class="truncate text-sm font-medium group-hover:underline" :title="itemName">
        {{ itemName }}
      </p>
      <div class="text-muted-foreground flex items-center gap-1.5 text-xs">
        <span class="min-w-0 truncate">{{ metaText }}</span>
        <!-- Owned counts: total (regular + foil), plus a foil count only when some are foil. -->
        <span v-if="ownedTotal > 0" class="flex shrink-0 items-center gap-1.5 tabular-nums">
          <span
            class="inline-flex items-center gap-0.5"
            :aria-label="`${ownedTotal} owned`"
            :title="`${ownedTotal} owned`"
          >
            <Layers class="size-3" aria-hidden="true" />{{ ownedTotal }}
          </span>
          <span
            v-if="foilCount > 0"
            class="inline-flex items-center gap-0.5"
            :aria-label="`${foilCount} foil`"
            :title="`${foilCount} foil`"
          >
            <Sparkles class="size-3" aria-hidden="true" />{{ foilCount }}
          </span>
        </span>
      </div>
    </div>
    <div class="shrink-0 text-right">
      <p class="text-sm font-semibold tabular-nums" :class="deltaClass">{{ changeText }}</p>
      <p class="mt-0.5 flex items-center justify-end gap-1.5">
        <span
          v-if="pctText"
          class="rounded-md px-1.5 py-0.5 text-[0.65rem] leading-none font-semibold tabular-nums"
          :class="chipClass"
        >
          {{ pctText }}
        </span>
        <span
          v-if="isFoilPrice"
          class="bg-foil/15 text-foil rounded-md px-1.5 py-0.5 text-[0.65rem] leading-none font-semibold"
        >
          Foil
        </span>
        <span v-if="priceNow" class="text-muted-foreground text-xs tabular-nums">
          {{ priceNow }}
        </span>
      </p>
    </div>
  </a>
</template>
