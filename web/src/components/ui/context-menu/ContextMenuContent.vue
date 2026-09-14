<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { ContextMenuContent, ContextMenuPortal } from 'reka-ui'
import { cn } from '@/lib/utils'

const props = defineProps<{ class?: HTMLAttributes['class'] }>()
</script>

<template>
  <ContextMenuPortal>
    <!-- A context menu is placed at the pointer, so unlike the dropdown there is no
      `align`/`sideOffset` to forward — reka positions it from the opening event. -->
    <ContextMenuContent
      data-slot="context-menu-content"
      :class="
        cn(
          'bg-popover text-popover-foreground data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 z-50 max-h-(--reka-context-menu-content-available-height) min-w-[9rem] origin-(--reka-context-menu-content-transform-origin) overflow-x-hidden overflow-y-auto rounded-md border p-1 shadow-md',
          props.class,
        )
      "
    >
      <slot />
    </ContextMenuContent>
  </ContextMenuPortal>
</template>
