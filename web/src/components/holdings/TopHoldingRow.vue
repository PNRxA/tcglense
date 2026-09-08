<script setup lang="ts">
import { computed } from 'vue'
import { Layers, Sparkles } from '@lucide/vue'
import CardImage from '@/components/cards/CardImage.vue'
import { useCurrency } from '@/composables/useCurrency'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import type { TopHolding } from '@/lib/api'
import type { CountNoun } from '@/lib/ownership'

// One row of the breakdown panel's top list (issue #680): the card, how many copies are
// held, and the held value (price × copies — the figure that ranks it). A click opens the
// shared card detail modal over the landing through the one `useDetailModalLink` seam every
// tile and row uses, keeping a real href for modifier / middle clicks.
const props = defineProps<{ game: string; holding: TopHolding; countNoun: CountNoun }>()
const money = useCurrency()
const { hrefFor, onActivate, warm } = useDetailModalLink()

const card = computed(() => props.holding.card)
const metaText = computed(
  () => `${card.value.set_code.toUpperCase()} · #${card.value.collector_number}`,
)
const total = computed(() => props.holding.quantity + props.holding.foil_quantity)
const foilCount = computed(() => props.holding.foil_quantity)
const value = computed(() => money.formatUsd(props.holding.value_usd))
</script>

<template>
  <a
    :href="hrefFor('card', game, card.id)"
    class="group hover:bg-muted/50 -mx-2 flex items-center gap-3 rounded-md px-2 py-1.5"
    @click="onActivate($event, 'card', game, card.id)"
    @pointerenter="warm('card')"
    @focusin="warm('card')"
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
