<script setup lang="ts">
import { computed } from 'vue'
import { Check, Loader2 } from '@lucide/vue'
import CardImage from '@/components/cards/CardImage.vue'
import { useCurrency } from '@/composables/useCurrency'
import { displayUsdPrice } from '@/lib/cardPrice'
import { printingMetadataLabel } from '@/lib/printings'
import type { Card } from '@/lib/api'

// Shared presentation for one exact card printing. Domain wrappers own the actions:
// collection/wish-list steppers live in the actions slot, deck add supplies its count/+1
// overlays, and deck replacement listens for select. This component owns only the common
// artwork, metadata, display-currency price, and selection/loading/disabled states.
const props = withDefaults(
  defineProps<{
    game: string
    card: Card
    selectable?: boolean
    current?: boolean
    loading?: boolean
    disabled?: boolean
    ariaLabel?: string
  }>(),
  {
    selectable: false,
    current: false,
    loading: false,
    disabled: false,
    ariaLabel: undefined,
  },
)
const emit = defineEmits<{ select: [] }>()

const money = useCurrency()
const price = computed(() => {
  const picked = displayUsdPrice(props.card.prices)
  return picked ? { ...picked, text: money.formatUsd(picked.amount) } : null
})
</script>

<template>
  <component
    :is="selectable ? 'button' : 'div'"
    :type="selectable ? 'button' : undefined"
    class="group focus-visible:ring-ring relative flex min-w-0 flex-col gap-2 rounded-lg border p-1.5 text-left outline-none transition"
    :class="{
      'hover:border-primary/50 focus-visible:ring-2': selectable && !disabled,
      'border-primary bg-primary/5': current,
      'cursor-not-allowed opacity-55': disabled && !current,
      'cursor-default': current,
    }"
    :disabled="selectable ? disabled : undefined"
    :aria-label="ariaLabel"
    :aria-busy="loading || undefined"
    @click="selectable && !disabled && emit('select')"
  >
    <div class="relative">
      <CardImage
        :game="game"
        :id="card.id"
        :name="card.name"
        :has-image="card.has_image"
        size="normal"
        class="w-full rounded-md"
      />

      <slot name="overlay" />

      <span
        v-if="current"
        class="bg-primary text-primary-foreground absolute right-1 bottom-1 z-10 flex items-center gap-1 rounded-md px-1.5 py-0.5 text-xs shadow"
      >
        <Check class="size-3" aria-hidden="true" /> Current
      </span>
      <span
        v-else-if="loading"
        class="bg-background/90 absolute right-1 bottom-1 z-10 rounded-full p-1.5 shadow"
      >
        <Loader2 class="size-4 animate-spin" aria-hidden="true" />
      </span>
    </div>

    <div class="min-w-0 px-0.5">
      <p class="truncate text-xs font-medium" :title="card.set_name">{{ card.set_name }}</p>
      <p class="text-muted-foreground flex flex-wrap items-center gap-x-1 text-xs">
        <span>{{ printingMetadataLabel(card) }}</span>
        <span v-if="price" class="tabular-nums"
          >· {{ price.text
          }}<span v-if="price.foil" class="ml-0.5 uppercase opacity-70">foil</span></span
        >
      </p>
    </div>

    <slot name="actions" />
  </component>
</template>
