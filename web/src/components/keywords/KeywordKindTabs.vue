<script setup lang="ts">
import { cn } from '@/lib/utils'
import type { KeywordKind } from '@/lib/api'

// The "All / Abilities / Actions / Ability words" segmented control on the glossary
// index, following GroupViewToggle's presentation. Each button carries its own count
// under the current text filter, so the taxonomy doubles as a result summary.
defineProps<{ counts: Record<KeywordKind | 'all', number> }>()

const selected = defineModel<KeywordKind | 'all'>({ required: true })

const TABS: { key: KeywordKind | 'all'; label: string }[] = [
  { key: 'all', label: 'All' },
  { key: 'ability', label: 'Abilities' },
  { key: 'action', label: 'Actions' },
  { key: 'ability_word', label: 'Ability words' },
]
</script>

<template>
  <div class="bg-muted text-muted-foreground inline-flex rounded-md p-0.5 text-sm">
    <button
      v-for="tab in TABS"
      :key="tab.key"
      type="button"
      :aria-pressed="selected === tab.key"
      :class="
        cn(
          'rounded px-3 py-1.5 font-medium transition-colors',
          selected === tab.key
            ? 'bg-background text-foreground shadow-sm'
            : 'hover:text-foreground',
        )
      "
      @click="selected = tab.key"
    >
      {{ tab.label }}
      <span class="ml-1 text-xs tabular-nums opacity-70">{{ counts[tab.key] }}</span>
    </button>
  </div>
</template>
