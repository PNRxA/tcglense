<script setup lang="ts">
import { computed } from 'vue'
import { ArrowDownUp, ChevronDown } from '@lucide/vue'
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
import type { SortOption } from '@/lib/cardSort'

const props = defineProps<{ options: SortOption[] }>()
// The selected option's `field:dir` value (see lib/cardSort).
const model = defineModel<string>({ required: true })

const activeLabel = computed(
  () => props.options.find((o) => o.value === model.value)?.label ?? 'Sort',
)

// Radio group hands back `string | undefined`; narrow before committing.
function onSelect(value: string | undefined) {
  if (value) model.value = value
}
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button variant="outline" size="sm" class="gap-2">
        <ArrowDownUp class="size-4" />
        <span class="truncate">{{ activeLabel }}</span>
        <ChevronDown class="size-4 opacity-60" />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end" class="w-52">
      <DropdownMenuLabel>Sort by</DropdownMenuLabel>
      <DropdownMenuSeparator />
      <DropdownMenuRadioGroup :model-value="model" @update:model-value="onSelect">
        <DropdownMenuRadioItem v-for="option in options" :key="option.value" :value="option.value">
          {{ option.label }}
        </DropdownMenuRadioItem>
      </DropdownMenuRadioGroup>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
