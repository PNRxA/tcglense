<script setup lang="ts">
import type { TooltipRootEmits, TooltipRootProps } from 'reka-ui'
import { TooltipRoot, useForwardPropsEmits } from 'reka-ui'
import TooltipProvider from './TooltipProvider.vue'

// Self-contained tooltip: it carries its own TooltipProvider so callers can drop a
// single <Tooltip> anywhere without wiring a provider at the app root (this app has
// no global one). Matches shadcn-vue's generated component.
const props = defineProps<TooltipRootProps>()
const emits = defineEmits<TooltipRootEmits>()

const forwarded = useForwardPropsEmits(props, emits)
</script>

<template>
  <TooltipProvider>
    <TooltipRoot data-slot="tooltip" v-bind="forwarded">
      <slot />
    </TooltipRoot>
  </TooltipProvider>
</template>
