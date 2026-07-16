<script setup lang="ts">
import { Undo2 } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import CardImage from '@/components/cards/CardImage.vue'
import type { SessionEntry } from '@/composables/useScanSession'

// The running tally of cards added this session, newest first — quick reassurance that
// scans are landing, and a one-tap undo for the inevitable misread.
defineProps<{
  game: string
  entries: SessionEntry[]
  disabled: boolean
}>()

const emit = defineEmits<{ undo: [number] }>()
</script>

<template>
  <ul class="divide-border divide-y">
    <li v-for="(entry, index) in entries" :key="entry.id" class="flex items-center gap-3 py-2">
      <CardImage
        :game="game"
        :id="entry.card.id"
        :name="entry.card.name"
        :has-image="entry.card.has_image"
        size="small"
        class="w-9 shrink-0"
      />
      <div class="min-w-0 flex-1">
        <p class="truncate text-sm font-medium">{{ entry.card.name }}</p>
        <p class="text-muted-foreground truncate text-xs">
          {{ entry.card.set_code.toUpperCase() }} · #{{ entry.card.collector_number }}
          <span class="tabular-nums">
            · {{ entry.quantity }} regular<template v-if="entry.foil_quantity">
              · {{ entry.foil_quantity }} foil</template
            >
          </span>
        </p>
      </div>
      <Button
        variant="ghost"
        size="sm"
        class="text-muted-foreground shrink-0"
        :disabled="disabled"
        :aria-label="`Undo adding ${entry.card.name}`"
        @click="emit('undo', index)"
      >
        <Undo2 class="size-4" aria-hidden="true" />
        Undo
      </Button>
    </li>
  </ul>
</template>
