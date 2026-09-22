<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { ContextMenuLabel } from 'reka-ui'
import { cn } from '@/lib/utils'

const props = defineProps<{ class?: HTMLAttributes['class'] }>()
</script>

<template>
  <ContextMenuLabel
    data-slot="context-menu-label"
    :class="cn('text-muted-foreground px-2 py-1.5 text-xs font-medium', props.class)"
  >
    <slot />
  </ContextMenuLabel>
</template>
