<script setup lang="ts">
import type { FunctionalComponent } from 'vue'
import { ChevronDown } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

// A single-select dropdown backed by a radio group, shared by the card size + sort
// menus. Options may carry an icon; the trigger shows its own icon plus the active
// label. The radio group hands back `string | undefined`, so that narrowing lives
// here once.
withDefaults(
  defineProps<{
    options: readonly { value: string; label: string; icon?: FunctionalComponent }[]
    label: string
    triggerIcon: FunctionalComponent
    triggerLabel: string
    align?: 'start' | 'center' | 'end'
    contentClass?: string
  }>(),
  { align: 'end', contentClass: undefined },
)

const model = defineModel<string>({ required: true })

function onSelect(value: string | undefined) {
  if (value) model.value = value
}
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button variant="outline" size="sm" class="gap-2">
        <component :is="triggerIcon" class="size-4" />
        <span class="truncate">{{ triggerLabel }}</span>
        <ChevronDown class="size-4 opacity-60" />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent :align="align" :class="contentClass">
      <DropdownMenuLabel>{{ label }}</DropdownMenuLabel>
      <DropdownMenuSeparator />
      <DropdownMenuRadioGroup :model-value="model" @update:model-value="onSelect">
        <DropdownMenuRadioItem v-for="option in options" :key="option.value" :value="option.value">
          <component :is="option.icon" v-if="option.icon" />
          {{ option.label }}
        </DropdownMenuRadioItem>
      </DropdownMenuRadioGroup>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
