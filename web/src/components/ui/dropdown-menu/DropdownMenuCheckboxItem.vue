<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { DropdownMenuCheckboxItem, DropdownMenuItemIndicator } from 'reka-ui'
import { Check } from '@lucide/vue'
import { cn } from '@/lib/utils'

// Hand-written to match the sibling DropdownMenuRadioItem's idiom (explicit props, no
// `useForwardPropsEmits`): a menu item with a check indicator gutter, bound to a boolean.
const props = defineProps<{
  class?: HTMLAttributes['class']
  modelValue?: boolean
  disabled?: boolean
}>()
const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  select: [event: Event]
}>()
</script>

<template>
  <DropdownMenuCheckboxItem
    data-slot="dropdown-menu-checkbox-item"
    :model-value="modelValue"
    :disabled="disabled"
    :class="
      cn(
        'focus:bg-accent focus:text-accent-foreground [&_svg:not([class*=text-])]:text-muted-foreground relative flex cursor-pointer items-center gap-2 rounded-sm py-1.5 pr-2 pl-8 text-sm outline-hidden transition-colors select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*=size-])]:size-4',
        props.class,
      )
    "
    @update:model-value="
      (value: boolean | 'indeterminate') => emit('update:modelValue', value === true)
    "
    @select="(event: Event) => emit('select', event)"
  >
    <span class="absolute left-2 flex items-center justify-center">
      <DropdownMenuItemIndicator>
        <Check class="size-4" />
      </DropdownMenuItemIndicator>
    </span>
    <slot />
  </DropdownMenuCheckboxItem>
</template>
