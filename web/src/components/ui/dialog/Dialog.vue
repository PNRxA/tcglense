<script setup lang="ts">
import { DialogRoot } from 'reka-ui'
</script>

<template>
  <!-- `open` / `v-model:open` / `default-open` fall through to the reka root. -->
  <DialogRoot data-slot="dialog">
    <slot />
  </DialogRoot>
</template>
