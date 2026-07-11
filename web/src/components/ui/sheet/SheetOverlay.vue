<script setup lang="ts">
import type { DialogOverlayProps } from 'reka-ui'
import type { HTMLAttributes } from 'vue'
import { computed } from 'vue'
import { DialogOverlay } from 'reka-ui'
import { cn } from '@/lib/utils'

const props = defineProps<DialogOverlayProps & { class?: HTMLAttributes['class'] }>()

// Strip the local-only `class` before forwarding to reka (no @vueuse dep here, so a
// computed omit stands in for reactiveOmit — same as the popover wrapper).
const delegatedProps = computed(() => {
  const { class: _class, ...rest } = props
  return rest
})
</script>

<template>
  <DialogOverlay
    data-slot="sheet-overlay"
    :class="
      cn(
        // motion-reduce: the tw-animate data-[state] fade does not respect
        // prefers-reduced-motion on its own.
        'data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 fixed inset-0 z-50 bg-black/80 motion-reduce:animate-none motion-reduce:transition-none',
        props.class,
      )
    "
    v-bind="delegatedProps"
  >
    <slot />
  </DialogOverlay>
</template>
