<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { ContextMenuPortal, ContextMenuSubContent } from 'reka-ui'
import { cn } from '@/lib/utils'

const props = defineProps<{ class?: HTMLAttributes['class'] }>()
</script>

<template>
  <ContextMenuPortal>
    <ContextMenuSubContent
      data-slot="context-menu-sub-content"
      :class="
        cn(
          'bg-popover text-popover-foreground data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 z-50 min-w-[9rem] origin-(--reka-context-menu-content-transform-origin) overflow-hidden rounded-md border p-1 shadow-lg',
          props.class,
        )
      "
    >
      <slot />
    </ContextMenuSubContent>
  </ContextMenuPortal>
</template>
