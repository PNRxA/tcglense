<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { DropdownMenuItemIndicator, DropdownMenuRadioItem } from 'reka-ui'
import { Check } from '@lucide/vue'
import { cn } from '@/lib/utils'

const props = defineProps<{
  class?: HTMLAttributes['class']
  value: string
  disabled?: boolean
}>()
</script>

<template>
  <DropdownMenuRadioItem
    data-slot="dropdown-menu-radio-item"
    :value="value"
    :disabled="disabled"
    :class="
      cn(
        'focus:bg-accent focus:text-accent-foreground [&_svg:not([class*=text-])]:text-muted-foreground relative flex cursor-pointer items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-hidden transition-colors select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*=size-])]:size-4',
        props.class,
      )
    "
  >
    <slot />
    <DropdownMenuItemIndicator class="ml-auto flex items-center">
      <Check class="size-4" />
    </DropdownMenuItemIndicator>
  </DropdownMenuRadioItem>
</template>
