<script setup lang="ts">
import type { PopoverRootEmits, PopoverRootProps } from 'reka-ui'
import { PopoverRoot, useForwardPropsEmits } from 'reka-ui'

// Thin shadcn-vue wrapper over reka's PopoverRoot: forwards props + emits so callers get
// `v-model:open`, and stamps a `data-slot` like the other ui primitives. Matches the
// generated shadcn-vue component (kept minimal — this app wires no global provider).
const props = defineProps<PopoverRootProps>()
const emits = defineEmits<PopoverRootEmits>()

const forwarded = useForwardPropsEmits(props, emits)
</script>

<template>
  <PopoverRoot data-slot="popover" v-bind="forwarded">
    <slot />
  </PopoverRoot>
</template>
