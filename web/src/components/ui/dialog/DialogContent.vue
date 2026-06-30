<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { DialogContent, DialogPortal } from 'reka-ui'
import { cn } from '@/lib/utils'
import DialogOverlay from './DialogOverlay.vue'

const props = defineProps<{ class?: HTMLAttributes['class'] }>()

// Two-element template (overlay + content); forward stray attrs/listeners to the
// content rather than letting them land on the portal.
defineOptions({ inheritAttrs: false })
</script>

<template>
  <DialogPortal>
    <DialogOverlay />
    <!-- Structural only: centered + animated, no visual chrome — callers supply
      their own sizing/background via `class` (a frameless image lightbox here, a
      padded panel elsewhere). -->
    <DialogContent
      data-slot="dialog-content"
      :class="
        cn(
          'data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 fixed top-1/2 left-1/2 z-50 -translate-x-1/2 -translate-y-1/2 focus:outline-none motion-reduce:animate-none',
          props.class,
        )
      "
      v-bind="$attrs"
    >
      <slot />
    </DialogContent>
  </DialogPortal>
</template>
