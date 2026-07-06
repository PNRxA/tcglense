<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { cn } from '@/lib/utils'

const props = defineProps<{ class?: HTMLAttributes['class'] }>()
</script>

<template>
  <!-- Pulsing placeholder for content that hasn't loaded. Static (no pulse) under
       reduced-motion. Callers pass sizing/shape via `class` (merged so a passed
       radius/colour overrides the base). -->
  <div
    data-slot="skeleton"
    :class="cn('bg-muted animate-pulse rounded-md motion-reduce:animate-none', props.class)"
    aria-hidden="true"
  />
</template>
