<script setup lang="ts">
import type { TooltipTriggerProps } from 'reka-ui'
import { TooltipTrigger } from 'reka-ui'

const props = defineProps<TooltipTriggerProps>()
</script>

<template>
  <!-- `as-child` falls through to the reka trigger so the badge chip itself becomes the
    trigger (no extra DOM), which matters here: the badge lives inside a RouterLink, so
    the trigger must not be its own <button>. -->
  <TooltipTrigger data-slot="tooltip-trigger" v-bind="props">
    <slot />
  </TooltipTrigger>
</template>
