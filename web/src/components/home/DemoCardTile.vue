<script setup lang="ts">
import { computed } from 'vue'
import { Layers, Plus, Sparkles } from '@lucide/vue'

// Presentational-only mock "card tile" used across the homepage's decorative marketing panels
// (hero vignette, Collection demo, Ghost-mode demo). Purely static illustration — no data flow.
const props = withDefaults(
  defineProps<{
    // Gradient header variant: the owned/primary tint or the neutral foreground→muted wash.
    gradient?: 'primary' | 'muted'
    // Whether to render the two skeleton "text" bars under the header.
    bars?: boolean
    // Ghost (missing) tile: dashed border + dimmed.
    ghost?: boolean
    // Owned-count badge value.
    layers?: number
    // Foil-count badge value.
    foil?: number
    // Crisp quick-add chip (a bare plus).
    quickAdd?: boolean
  }>(),
  {
    gradient: 'primary',
    bars: true,
    ghost: false,
    quickAdd: false,
  },
)

// The owned-count / quick-add chip style, verbatim from OwnedCountBadge.vue so the demo
// badges match the real ones.
const badgeChipClass =
  'bg-primary text-primary-foreground inline-flex items-center gap-0.5 rounded-md px-1.5 ' +
  'py-0.5 text-xs font-semibold shadow tabular-nums'

const cardClass = computed(() =>
  props.ghost
    ? 'bg-muted aspect-[5/7] overflow-hidden rounded-lg border border-dashed opacity-40'
    : 'bg-muted aspect-[5/7] overflow-hidden rounded-lg border',
)

const gradientClass = computed(() =>
  props.gradient === 'primary'
    ? 'from-primary/25 via-primary/10 h-2/5 bg-gradient-to-br to-transparent'
    : 'from-foreground/10 h-2/5 bg-gradient-to-br via-transparent to-muted',
)

const hasBadges = computed(() => props.layers != null || props.foil != null || props.quickAdd)
</script>

<template>
  <div class="relative">
    <div :class="cardClass">
      <div :class="gradientClass"></div>
      <div v-if="bars" class="space-y-1.5 p-2">
        <div class="bg-foreground/15 h-1.5 w-3/4 rounded-full"></div>
        <div class="bg-foreground/10 h-1.5 w-1/2 rounded-full"></div>
      </div>
    </div>
    <div v-if="hasBadges" class="absolute bottom-1.5 left-1.5 flex items-center gap-1">
      <span v-if="layers != null" :class="badgeChipClass">
        <Layers class="size-3" aria-hidden="true" />
        {{ layers }}
      </span>
      <span v-if="foil != null" :class="badgeChipClass">
        <Sparkles class="size-3" aria-hidden="true" />
        {{ foil }}
      </span>
      <span v-if="quickAdd" :class="badgeChipClass">
        <Plus class="size-3" aria-hidden="true" />
      </span>
    </div>
  </div>
</template>
