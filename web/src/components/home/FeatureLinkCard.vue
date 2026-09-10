<script setup lang="ts">
import type { Component } from 'vue'
import { ArrowUpRight, ChevronRight } from '@lucide/vue'
import { RouterLink } from 'vue-router'

// One linked feature on the homepage's "everything else" section, in one of two weights:
// a `card` (the headline features — a tinted icon well, the title with a corner affordance,
// and a description, stacked) or a `row` (the rest — a small muted well beside a two-line
// label, no border). Both weights share the one link switch: `href` renders an <a> that
// opens in a new tab and shows the outbound arrow, `to` renders a RouterLink and shows the
// chevron — so a feature is an array entry on the view, never a hand-written anchor.
export interface FeatureLink {
  icon: Component
  title: string
  description: string
  // Exactly one of the two.
  to?: string
  href?: string
}

withDefaults(
  defineProps<{
    feature: FeatureLink
    variant?: 'card' | 'row'
  }>(),
  { variant: 'card' },
)
</script>

<template>
  <component
    :is="feature.href ? 'a' : RouterLink"
    v-bind="
      feature.href
        ? { href: feature.href, target: '_blank', rel: 'noopener noreferrer' }
        : { to: feature.to }
    "
    class="group transition-colors"
    :class="
      variant === 'card'
        ? 'bg-card hover:border-ring/60 hover:bg-accent/40 shadow-card hover:shadow-lift flex flex-col gap-3 rounded-xl border p-5 transition-shadow'
        : 'hover:bg-accent/40 flex items-start gap-3 rounded-lg px-3 py-2.5'
    "
  >
    <template v-if="variant === 'card'">
      <span class="flex items-start justify-between gap-3">
        <span
          class="bg-primary/10 text-primary flex size-10 shrink-0 items-center justify-center rounded-lg"
        >
          <component :is="feature.icon" class="size-5" aria-hidden="true" />
        </span>
        <component
          :is="feature.href ? ArrowUpRight : ChevronRight"
          class="text-muted-foreground group-hover:text-primary size-4 shrink-0 transition-colors"
          aria-hidden="true"
        />
      </span>
      <span class="min-w-0">
        <span class="text-foreground block font-medium">{{ feature.title }}</span>
        <span class="text-muted-foreground mt-1 block text-sm text-pretty">
          {{ feature.description }}
        </span>
      </span>
    </template>
    <template v-else>
      <span
        class="bg-muted text-muted-foreground group-hover:text-primary flex size-8 shrink-0 items-center justify-center rounded-md transition-colors"
      >
        <component :is="feature.icon" class="size-4" aria-hidden="true" />
      </span>
      <span class="min-w-0">
        <span class="text-foreground block text-sm font-medium">{{ feature.title }}</span>
        <span class="text-muted-foreground mt-0.5 block text-sm text-pretty">
          {{ feature.description }}
        </span>
      </span>
      <component
        :is="feature.href ? ArrowUpRight : ChevronRight"
        class="text-muted-foreground mt-2 ml-auto size-4 shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
        aria-hidden="true"
      />
    </template>
  </component>
</template>
