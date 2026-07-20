<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { Boxes, Layers, Package, Sparkles } from '@lucide/vue'
import {
  useProductCardSectionsQuery,
  useProductContainersQuery,
  useProductContentsQuery,
} from '@/composables/useProducts'
import { boosterFamilyLabel } from '@/lib/productType'

// The sealed product's at-a-glance strip: how many items its box holds, how many cards
// it contains or can pull (with the booster-family exclusives called out), and how many
// parent products bundle it — each chip a jump link to the matching section further down
// the page, so what's buried deep is surfaced at the top (the "cards in this product"
// list especially). Every count rides a query key a section below shares — the contents
// list, the card-sections manifest (unfiltered), the containers list — so this strip
// adds no fetch of its own. Renders nothing when no count is known (yet).
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

const emit = defineEmits<{ jump: [target: 'contents' | 'cards' | 'containers'] }>()

const contentsQuery = useProductContentsQuery(game, id)
// Unfiltered manifest — the same key ProductCards starts from (no committed search), so
// this reads the cached response rather than refetching.
const sectionsQuery = useProductCardSectionsQuery(game, id, ref(''))
const containersQuery = useProductContainersQuery(game, id)

const boxItems = computed(() => contentsQuery.data.value?.data.length ?? 0)
const manifest = computed(() => sectionsQuery.data.value?.data ?? [])
const cardTotal = computed(() => manifest.value.reduce((sum, s) => sum + s.total, 0))
const exclusiveSection = computed(() => manifest.value.find((s) => s.key === 'exclusive'))
const exclusiveLabel = computed(() => {
  const family = exclusiveSection.value?.booster_family
  const name = family ? boosterFamilyLabel(family) : null
  return name ? `${name} exclusives` : 'booster exclusives'
})
const containerCount = computed(() => containersQuery.data.value?.data.length ?? 0)

const chips = computed(() =>
  [
    {
      key: 'contents' as const,
      icon: Package,
      count: boxItems.value,
      label: boxItems.value === 1 ? 'item in the box' : 'items in the box',
    },
    {
      key: 'cards' as const,
      icon: Layers,
      count: cardTotal.value,
      label: cardTotal.value === 1 ? 'card inside' : 'cards inside',
    },
    {
      key: 'cards' as const,
      icon: Sparkles,
      count: exclusiveSection.value?.total ?? 0,
      label: exclusiveLabel.value,
    },
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
      :title="`Jump to ${chip.count.toLocaleString()} ${chip.label}`"
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
