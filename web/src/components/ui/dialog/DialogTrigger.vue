<script setup lang="ts">
import { DialogTrigger } from 'reka-ui'
</script>

<template>
  <!-- `as-child`, `class`, and aria-* fall through to the reka trigger. -->
  <DialogTrigger data-slot="dialog-trigger">
    <slot />
  </DialogTrigger>
</template>
