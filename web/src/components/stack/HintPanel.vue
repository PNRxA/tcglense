<script setup lang="ts">
import { Lightbulb } from '@lucide/vue'
import RuleChip from '@/components/stack/RuleChip.vue'
import type { Hint } from '@/lib/stack/hints'

// "What happens next": the situation in one paragraph, plus the refusal from the last action
// when there was one — the reason a button didn't do what the player expected is the lesson.
defineProps<{ hint: Hint; refusal: string | null }>()
</script>

<template>
  <section class="bg-info/10 border-info/30 rounded-xl border p-4" aria-live="polite">
    <p class="flex items-center gap-2 text-sm font-semibold">
      <Lightbulb class="text-info size-4" aria-hidden="true" /> What happens next
    </p>
    <p class="mt-1.5 text-sm leading-relaxed" data-testid="hint">{{ hint.text }}</p>
    <RuleChip v-if="hint.rule" :rule="hint.rule" class="mt-2" />
    <p
      v-if="refusal"
      class="text-destructive border-destructive/30 mt-3 border-t pt-3 text-sm leading-relaxed"
      data-testid="refusal"
    >
      {{ refusal }}
    </p>
  </section>
</template>
