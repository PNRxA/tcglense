<script setup lang="ts">
import { KIND_LABELS } from '@/lib/keywords'
import type { KeywordKind } from '@/lib/api'

// The "Keyword ability / Keyword action / Ability word" chip. Tinted-translucent so it
// reads in both themes, the same idiom CardLegalities' status chips use.
defineProps<{ kind: KeywordKind }>()

const KIND_CHIP_CLASSES: Record<KeywordKind, string> = {
  ability: 'bg-sky-500/15 text-sky-700 dark:text-sky-400',
  action: 'bg-amber-500/15 text-amber-700 dark:text-amber-400',
  ability_word: 'bg-violet-500/15 text-violet-700 dark:text-violet-400',
}
</script>

<template>
  <span
    class="inline-flex items-center rounded px-2 py-0.5 text-xs font-medium"
    :class="KIND_CHIP_CLASSES[kind]"
  >
    {{ KIND_LABELS[kind] }}
  </span>
</template>
