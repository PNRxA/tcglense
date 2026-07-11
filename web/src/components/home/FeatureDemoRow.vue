<script setup lang="ts">
import type { Component } from 'vue'

// The shared shell for the homepage's five feature-demo rows: a text column (eyebrow, heading,
// body, CTA slot) paired with a decorative mock panel that alternates sides at md+. The panel
// markup is passed through the named "demo" slot so nothing about the rendered SVG/values changes.
withDefaults(
  defineProps<{
    icon: Component
    eyebrow: string
    heading: string
    body: string
    // Which side the demo panel sits on at md+ (left = md:order-first).
    demoSide?: 'left' | 'right'
  }>(),
  {
    demoSide: 'right',
  },
)
</script>

<template>
  <div class="grid items-center gap-8 md:grid-cols-2 md:gap-12">
    <div>
      <p class="text-primary flex items-center gap-2 text-sm font-semibold">
        <component :is="icon" class="size-4" aria-hidden="true" />
        {{ eyebrow }}
      </p>
      <h2 class="mt-3 text-2xl font-semibold tracking-tight text-balance sm:text-3xl">
        {{ heading }}
      </h2>
      <p class="text-muted-foreground mt-3 text-pretty">{{ body }}</p>
      <div class="mt-5 flex flex-wrap gap-x-6 gap-y-2">
        <slot />
      </div>
    </div>
    <div
      class="bg-card rounded-2xl border p-5 sm:p-6"
      :class="demoSide === 'left' ? 'md:order-first' : ''"
      aria-hidden="true"
    >
      <slot name="demo" />
    </div>
  </div>
</template>
