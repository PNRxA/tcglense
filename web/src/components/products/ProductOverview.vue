<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { Boxes, Dices, Layers, Package, Shuffle, Sparkles } from '@lucide/vue'
import {
  useProductCardSectionsQuery,
  useProductContainersQuery,
  useProductContentsQuery,
} from '@/composables/useProducts'
import { boosterFamilyLabel } from '@/lib/productType'
import {
  boxItemCount,
  productCardChips,
  productCardCounts,
  visibleProductSections,
} from '@/lib/productCounts'

// The sealed product's at-a-glance strip: how many pieces its box holds, how many cards are
// guaranteed, how many are only in the booster pull pool (with the booster-family exclusives
// called out as a slice of that pool), how many a randomized configuration might add, and how
// many parent products bundle it — each chip a jump link to the matching section further down
// the page, so what's buried deep is surfaced at the top (the card list especially). The card
// chips are split by certainty rather than summed: one "N cards inside" chip announced a
// booster's whole pull pool as its contents. Every count rides a query key a section below
// shares — the contents list, the card-sections manifest (unfiltered), the containers list —
// so this strip adds no fetch of its own. Renders nothing when no count is known (yet).
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

const emit = defineEmits<{ jump: [target: 'contents' | 'cards' | 'containers'] }>()

const contentsQuery = useProductContentsQuery(game, id)
// Unfiltered manifest — the same key ProductCards starts from (no committed search), so
// this reads the cached response rather than refetching.
const sectionsQuery = useProductCardSectionsQuery(game, id, ref(''))
const containersQuery = useProductContainersQuery(game, id)

// Physical pieces, not line items: a booster box is one `30× pack` row plus a topper — 31
// items, not 2. Shares boxItemCount with ProductContents' own heading so the two agree.
const boxItems = computed(() => boxItemCount(contentsQuery.data.value?.data ?? []))
// The *visible* manifest — the same filter ProductCards renders through, so a chip can
// never count a pool the sections below have hidden (an inherited booster/exclusive
// section defers to the listed sub-product's own page).
const manifest = computed(() => visibleProductSections(sectionsQuery.data.value?.data ?? []))
const counts = computed(() => productCardCounts(manifest.value))
// The exclusives' booster family, when the backend names one. This strip takes no
// `product_type`, so there's no own-family fallback — the chip goes generic instead.
const exclusiveFamily = computed(() => {
  const family = manifest.value.find((s) => !s.component && s.key === 'exclusive')?.booster_family
  return family ? boosterFamilyLabel(family) : null
})
const CHIP_ICONS = { guaranteed: Layers, pull: Dices, exclusive: Sparkles, variable: Shuffle }
const containerCount = computed(() => containersQuery.data.value?.data.length ?? 0)

type OverviewChip = {
  key: 'contents' | 'cards' | 'containers'
  icon: unknown
  count: number
  label: string
  // Extends the "Jump to …" tooltip where the label alone could still mislead (a pool size read
  // as a pack's worth, a distinct-card count read as copies).
  hint?: string
}

// Certainty descends left to right — box pieces, guaranteed cards, the pull pool, the pool's
// exclusive slice, randomized maybes, then the parents that bundle this product. Every card
// label is self-contained (productCardChips), so the strip can wrap anywhere without a chip
// losing the set it describes.
const chips = computed<OverviewChip[]>(() =>
  [
    {
      key: 'contents' as const,
      icon: Package,
      count: boxItems.value,
      label: boxItems.value === 1 ? 'item in the box' : 'items in the box',
    },
    ...productCardChips(counts.value, exclusiveFamily.value).map((chip) => ({
      key: 'cards' as const,
      icon: CHIP_ICONS[chip.id],
      count: chip.count,
      label: chip.label,
      hint: chip.hint,
    })),
    {
      key: 'containers' as const,
      icon: Boxes,
      count: containerCount.value,
      label: containerCount.value === 1 ? 'product includes this' : 'products include this',
    },
  ].filter((chip) => chip.count > 0),
)
</script>

<template>
  <div v-if="chips.length" class="flex flex-wrap gap-2">
    <button
      v-for="(chip, i) in chips"
      :key="i"
      type="button"
      class="bg-card hover:bg-muted/50 inline-flex items-center gap-2 rounded-lg border px-3 py-2 text-left text-sm shadow-sm transition-colors"
      :title="`Jump to ${chip.count.toLocaleString()} ${chip.label}${chip.hint ? ` — ${chip.hint}` : ''}`"
      @click="emit('jump', chip.key)"
    >
      <component :is="chip.icon" class="text-muted-foreground size-4 shrink-0" aria-hidden="true" />
      <span class="flex items-baseline gap-1">
        <span class="font-semibold tabular-nums">{{ chip.count.toLocaleString() }}</span>
        <span class="text-muted-foreground">{{ chip.label }}</span>
      </span>
    </button>
  </div>
</template>
