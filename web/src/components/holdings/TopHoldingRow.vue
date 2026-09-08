<script lang="ts">
// Warm the shared card-detail dialog chunk on the first hover/focus of ANY row (module
// flag → once per session), mirroring MoverRow, so the click that opens ?card= finds the
// chunk already fetched.
let dialogWarmed = false
</script>

<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { Layers, Sparkles } from '@lucide/vue'
import CardImage from '@/components/cards/CardImage.vue'
import { loadCardDetailDialog } from '@/components/cards/detailDialogLoader'
import { useCurrency } from '@/composables/useCurrency'
import type { TopHolding } from '@/lib/api'

// One row of the breakdown panel's "Top holdings" list (issue #680): the card, how many
// copies are held, and the held value (price × copies — the figure that ranks it). Clicks
// open the shared card detail modal over the landing, keeping a real href for modifier /
// middle clicks — the same idiom as the movers panel's rows.
const props = defineProps<{ game: string; holding: TopHolding; countNoun: string }>()
const money = useCurrency()
const route = useRoute()
const router = useRouter()

const card = computed(() => props.holding.card)
const metaText = computed(
  () => `${card.value.set_code.toUpperCase()} · #${card.value.collector_number}`,
)
const total = computed(() => props.holding.quantity + props.holding.foil_quantity)
const foilCount = computed(() => props.holding.foil_quantity)
const value = computed(() => money.formatUsd(props.holding.value_usd))

const to = computed(() => `/cards/${props.game}/cards/${card.value.id}`)
const href = computed(() => router.resolve(to.value).href)
function onClick(event: MouseEvent) {
  if (event.defaultPrevented) return
  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
    return
  }
  event.preventDefault()
  void router.push({ query: { ...route.query, card: card.value.id } })
}
function warmDetail() {
  if (!dialogWarmed) {
    dialogWarmed = true
    void loadCardDetailDialog()
  }
}
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
      :game="game"
      :id="card.id"
      :name="card.name"
      :has-image="card.has_image"
      size="small"
      class="w-10 shrink-0"
    />
    <div class="min-w-0 flex-1">
      <p class="truncate text-sm font-medium group-hover:underline" :title="card.name">
        {{ card.name }}
      </p>
      <div class="text-muted-foreground flex items-center gap-1.5 text-xs">
        <span class="min-w-0 truncate">{{ metaText }}</span>
        <span class="flex shrink-0 items-center gap-1.5 tabular-nums">
          <span
            class="inline-flex items-center gap-0.5"
            :aria-label="`${total} ${countNoun}`"
            :title="`${total} ${countNoun}`"
          >
            <Layers class="size-3" aria-hidden="true" />{{ total }}
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
    <p class="shrink-0 text-right text-sm font-semibold tabular-nums">{{ value }}</p>
  </a>
</template>
