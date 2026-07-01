<script setup lang="ts">
import { computed, type FunctionalComponent } from 'vue'
import { Monitor, Moon, Sun } from '@lucide/vue'
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
import { useThemeStore, type Theme } from '@/stores/theme'

const theme = useThemeStore()

const options: { value: Theme; label: string; icon: FunctionalComponent }[] = [
  { value: 'light', label: 'Light', icon: Sun },
  { value: 'dark', label: 'Dark', icon: Moon },
  { value: 'system', label: 'System', icon: Monitor },
]

// The trigger reflects what's actually on screen: a sun in light, a moon in dark.
const triggerIcon = computed(() => (theme.resolvedTheme === 'dark' ? Moon : Sun))

function onSelect(value: string | undefined) {
  if (value === 'light' || value === 'dark' || value === 'system') {
    theme.setTheme(value)
  }
}
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button variant="ghost" size="icon">
        <component :is="triggerIcon" />
        <span class="sr-only">Toggle theme</span>
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end" class="w-40">
      <DropdownMenuLabel>Theme</DropdownMenuLabel>
      <DropdownMenuSeparator />
      <DropdownMenuRadioGroup :model-value="theme.theme" @update:model-value="onSelect">
        <DropdownMenuRadioItem v-for="option in options" :key="option.value" :value="option.value">
          <component :is="option.icon" />
          {{ option.label }}
        </DropdownMenuRadioItem>
      </DropdownMenuRadioGroup>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
