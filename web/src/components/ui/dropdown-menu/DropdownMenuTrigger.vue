<script setup lang="ts">
import { DropdownMenuTrigger } from 'reka-ui'
</script>

<template>
  <!-- `as-child` falls through to the reka trigger so a <Button> can be the trigger. -->
  <DropdownMenuTrigger data-slot="dropdown-menu-trigger">
    <slot />
  </DropdownMenuTrigger>
</template>
