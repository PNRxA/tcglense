<script setup lang="ts">
import { Switch } from '@/components/ui/switch'
import { COUNTER_META, LIFE_COUNTERS, type LifeCounterKind } from '@/lib/lifeCounters'

// Which counters a game tracks.
//
// Per game rather than per account, because it is a property of what is being played: a Standard
// pod has no business seeing a commander-damage matrix, and the same person plays both. The
// server derives a sensible default from the format (a Commander pod opens with the matrix on),
// so this is a correction rather than a chore.
//
// Life is not on the list — it is always tracked, and offering it as a toggle would suggest
// otherwise.
const model = defineModel<LifeCounterKind[]>({ required: true })

defineProps<{ idPrefix: string }>()

function toggle(kind: LifeCounterKind, on: boolean) {
  // Rebuilt in vocabulary order rather than push/splice, so the list the API gets is the order
  // it stores and the rows never reorder under the user.
  model.value = LIFE_COUNTERS.filter((known) => (known === kind ? on : model.value.includes(known)))
}
</script>

<template>
  <div class="space-y-3">
    <div v-for="kind in LIFE_COUNTERS" :key="kind" class="flex items-start gap-3">
      <div class="min-w-0 flex-1">
        <p :id="`${idPrefix}-${kind}`" class="text-sm font-medium">
          {{ COUNTER_META[kind].label }}
        </p>
        <p class="text-muted-foreground text-xs">{{ COUNTER_META[kind].hint }}</p>
      </div>
      <Switch
        :checked="model.includes(kind)"
        :aria-labelledby="`${idPrefix}-${kind}`"
        class="mt-0.5 shrink-0"
        @update:checked="(on: boolean) => toggle(kind, on)"
      />
    </div>
  </div>
</template>
