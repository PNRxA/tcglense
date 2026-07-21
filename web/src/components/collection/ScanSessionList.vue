<script setup lang="ts">
import { computed, ref, useId, watch } from 'vue'
import { Undo2 } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import CardImage from '@/components/cards/CardImage.vue'
import type { SessionEntry } from '@/composables/useScanSession'

// The running tally of cards added this session, newest first — quick reassurance that
// scans are landing, and a one-tap undo for the inevitable misread.
const props = defineProps<{
  game: string
  entries: SessionEntry[]
  disabled: boolean
}>()

const emit = defineEmits<{ undo: [number] }>()
const listId = useId()
const expanded = ref(false)
const visibleEntries = computed(() => (expanded.value ? props.entries : props.entries.slice(0, 3)))

watch(
  () => props.entries.length,
  (length) => {
    if (length <= 3) expanded.value = false
  },
)
</script>

<template>
  <ul :id="listId" class="divide-border divide-y">
    <li
      v-for="(entry, index) in visibleEntries"
      :key="entry.id"
      class="flex min-w-0 items-center gap-3 py-2"
    >
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
            · Now {{ entry.quantity }} regular<template v-if="entry.foil_quantity">
              · {{ entry.foil_quantity }} foil</template
            >
          </span>
        </p>
      </div>
      <Button
        variant="ghost"
        size="sm"
        class="text-muted-foreground min-h-11 shrink-0 lg:min-h-8"
        :disabled="disabled"
        :aria-label="`Undo adding ${entry.card.name}`"
        @click="emit('undo', index)"
      >
        <Undo2 class="size-4" aria-hidden="true" />
        Undo
      </Button>
    </li>
  </ul>

  <Button
    v-if="entries.length > 3"
    variant="ghost"
    size="sm"
    class="mt-1 min-h-11 w-full lg:min-h-8"
    :aria-expanded="expanded"
    :aria-controls="listId"
    @click="expanded = !expanded"
  >
    {{ expanded ? 'Show less' : `View all (${entries.length})` }}
  </Button>
</template>
