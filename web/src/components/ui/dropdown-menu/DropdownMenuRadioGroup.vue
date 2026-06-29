<script setup lang="ts">
import { DropdownMenuRadioGroup } from 'reka-ui'

const modelValue = defineModel<string>()
</script>

<template>
  <DropdownMenuRadioGroup v-model="modelValue" data-slot="dropdown-menu-radio-group">
    <slot />
  </DropdownMenuRadioGroup>
</template>
