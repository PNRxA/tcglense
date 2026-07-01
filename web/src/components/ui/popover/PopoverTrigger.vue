<script setup lang="ts">
import type { PopoverTriggerProps } from 'reka-ui'
import { PopoverTrigger } from 'reka-ui'

const props = defineProps<PopoverTriggerProps>()
</script>

<template>
  <!-- `as-child` falls through to the caller's element (e.g. the owned-count chip button)
    so the trigger adds no wrapper DOM and the button itself carries the popover a11y. -->
  <PopoverTrigger data-slot="popover-trigger" v-bind="props">
    <slot />
  </PopoverTrigger>
</template>
