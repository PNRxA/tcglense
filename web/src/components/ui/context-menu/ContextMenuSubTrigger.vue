<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { ContextMenuSubTrigger } from 'reka-ui'
import { ChevronRight } from '@lucide/vue'
import { cn } from '@/lib/utils'

const props = defineProps<{ class?: HTMLAttributes['class']; disabled?: boolean }>()
</script>

<template>
  <ContextMenuSubTrigger
    data-slot="context-menu-sub-trigger"
    :disabled="disabled"
    :class="
      cn(
        'focus:bg-accent focus:text-accent-foreground data-[state=open]:bg-accent data-[state=open]:text-accent-foreground [&_svg:not([class*=text-])]:text-muted-foreground flex cursor-pointer items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-hidden select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*=size-])]:size-4',
        props.class,
      )
    "
  >
    <slot />
    <ChevronRight class="ml-auto" />
  </ContextMenuSubTrigger>
</template>
