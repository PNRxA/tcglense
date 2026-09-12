<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import RuleChip from '@/components/stack/RuleChip.vue'
import type { LogEntry, LogKind } from '@/lib/stack/types'

// The narrated history: every line the engine wrote, oldest first so it reads as a story,
// with the lines the last action added picked out. Each line that rests on a rule carries
// the citation chip, so "why" is always one tap away.
const props = defineProps<{ entries: LogEntry[]; latest: LogEntry[] }>()

const KIND_CLASS: Record<LogKind, string> = {
  action: 'border-l-primary',
  pass: 'border-l-muted-foreground/40',
  resolve: 'border-l-success',
  trigger: 'border-l-warning',
  sba: 'border-l-warning',
  refused: 'border-l-destructive bg-destructive/5',
  note: 'border-l-info/60',
}

const KIND_LABEL: Record<LogKind, string> = {
  action: 'Action',
  pass: 'Priority',
  resolve: 'Resolves',
  trigger: 'Trigger',
  sba: 'State-based action',
  refused: 'Not allowed',
  note: 'Note',
}

const latestIds = computed(() => new Set(props.latest.map((entry) => entry.id)))
const scroller = ref<HTMLElement | null>(null)

watch(
  () => props.entries.length,
  async () => {
    await nextTick()
    const el = scroller.value
    if (el) el.scrollTop = el.scrollHeight
  },
)
</script>

<template>
  <section class="bg-card rounded-xl border p-4 shadow-sm" aria-labelledby="log-heading">
    <h2 id="log-heading" class="text-sm font-semibold">What happened, and why</h2>
    <p v-if="entries.length === 0" class="text-muted-foreground mt-2 text-sm">
      Nothing yet. Every action writes its explanation here.
    </p>
    <ol
      v-else
      ref="scroller"
      class="mt-2 max-h-[28rem] space-y-1.5 overflow-y-auto pr-1"
      aria-live="polite"
      data-testid="stack-log"
    >
      <li
        v-for="entry in entries"
        :key="entry.id"
        class="rounded-r-md border-l-2 py-1.5 pl-3 text-sm leading-relaxed transition-colors"
        :class="[
          KIND_CLASS[entry.kind],
          latestIds.has(entry.id) ? 'bg-accent/40' : 'text-muted-foreground',
        ]"
      >
        <span class="text-[0.65rem] font-medium tracking-wide uppercase opacity-70">{{
          KIND_LABEL[entry.kind]
        }}</span>
        <span class="block">{{ entry.text }}</span>
        <RuleChip v-if="entry.rule" :rule="entry.rule" class="mt-1" />
      </li>
    </ol>
  </section>
</template>
