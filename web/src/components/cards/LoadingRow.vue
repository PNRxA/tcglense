<script setup lang="ts">
import { Loader2 } from '@lucide/vue'

// A muted loading row with a spinner, shared by the catalog views' pending state.
// `label` names what's loading (e.g. "Loading cards…"); a default slot can override.
defineProps<{ label?: string }>()
</script>

<template>
  <div class="text-muted-foreground flex items-center gap-2 py-12">
    <Loader2 class="size-4 animate-spin" />
    <slot>{{ label }}</slot>
  </div>
</template>
