<script setup lang="ts">
import { computed } from 'vue'
import type { Card } from '@/lib/api'
import { legalityLabel, MTG_FORMATS, statusOf, type LegalityStatus } from '@/lib/legality'

const props = defineProps<{
  card: Card
}>()

const STATUS_CHIP_CLASSES: Record<LegalityStatus, string> = {
  legal: 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-400',
  not_legal: 'bg-muted text-muted-foreground',
  banned: 'bg-red-500/15 text-red-700 dark:text-red-400',
  restricted: 'bg-amber-500/15 text-amber-700 dark:text-amber-400',
}

const formats = computed(() =>
  MTG_FORMATS.map((format) => {
    const status = statusOf(props.card, format.key)
    return {
      ...format,
      status,
      statusLabel: status == null ? '—' : legalityLabel(status),
      statusClass: status == null ? 'bg-muted text-muted-foreground' : STATUS_CHIP_CLASSES[status],
    }
  }),
)
</script>

<template>
  <div v-if="card.legalities !== null" class="bg-card rounded-xl border p-4 shadow-sm">
    <h2 class="mb-3 text-sm font-semibold">Format legality</h2>
    <div class="grid grid-cols-2 gap-x-4 gap-y-2 sm:grid-cols-3">
      <div
        v-for="format in formats"
        :key="format.key"
        :data-format="format.key"
        class="flex min-w-0 items-center gap-2"
      >
        <span
          class="inline-flex w-20 shrink-0 items-center justify-center rounded px-2 py-0.5 text-xs font-medium"
          :class="format.statusClass"
        >
          {{ format.statusLabel }}
        </span>
        <span class="min-w-0 text-sm">{{ format.label }}</span>
      </div>
    </div>
  </div>
</template>
