<script setup lang="ts">
import { computed } from 'vue'
import { ChevronDown, LayoutGrid } from '@lucide/vue'
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
import { CARD_SIZE_OPTIONS, isCardSize } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'

// Self-contained like ThemeToggle: it reads and writes the shared preference store
// directly, so a view only has to drop <CardSizeMenu /> into its toolbar.
const cardSize = useCardSizeStore()

const activeLabel = computed(
  () => CARD_SIZE_OPTIONS.find((o) => o.value === cardSize.size)?.label ?? 'Size',
)

// The radio group hands back `string | undefined`; narrow before committing.
function onSelect(value: string | undefined) {
  if (isCardSize(value)) cardSize.setSize(value)
}
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button variant="outline" size="sm" class="gap-2">
        <LayoutGrid class="size-4" />
        <span class="truncate">{{ activeLabel }}</span>
        <ChevronDown class="size-4 opacity-60" />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end" class="w-44">
      <DropdownMenuLabel>Card size</DropdownMenuLabel>
      <DropdownMenuSeparator />
      <DropdownMenuRadioGroup :model-value="cardSize.size" @update:model-value="onSelect">
        <DropdownMenuRadioItem
          v-for="option in CARD_SIZE_OPTIONS"
          :key="option.value"
          :value="option.value"
        >
          {{ option.label }}
        </DropdownMenuRadioItem>
      </DropdownMenuRadioGroup>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
