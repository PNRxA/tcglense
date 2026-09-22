<script setup lang="ts">
import { ContextMenuTrigger } from 'reka-ui'

const props = withDefaults(defineProps<{ disabled?: boolean }>(), { disabled: false })
</script>

<template>
  <!-- `as-child` falls through to the reka trigger so the card button itself is the target. -->
  <ContextMenuTrigger data-slot="context-menu-trigger" :disabled="props.disabled" as-child>
    <slot />
  </ContextMenuTrigger>
</template>
