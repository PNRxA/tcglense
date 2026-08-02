<script setup lang="ts">
import { computed } from 'vue'
import { CircleCheck, TriangleAlert } from '@lucide/vue'
import { DECK_ISSUE_STATUSES, deckIssueLabel } from '@/lib/legality'
import type { DeckIssueStatus, DeckLegality, DeckLegalityIssue } from '@/lib/api'

const props = defineProps<{
  legality: DeckLegality
}>()

// Errors first, then the "you aren't finished building" warnings — a deck that is both
// 40 cards short and running an off-colour card should lead with the illegal part.
const violations = computed(() =>
  [...props.legality.violations].sort(
    (a, b) => Number(b.severity === 'error') - Number(a.severity === 'error'),
  ),
)
/**
 * `error` = illegal as it stands (red), `warning` = only unfinished (amber), `clean` = the
 * quiet all-clear line. A half-built deck shouldn't be shouted at in red.
 */
const state = computed<'error' | 'warning' | 'clean'>(() => {
  if (!props.legality.legal) return 'error'
  return violations.value.length > 0 ? 'warning' : 'clean'
})

const visibleIssues = computed(() => props.legality.issues.slice(0, 8))
const hiddenIssueCount = computed(() => Math.max(0, props.legality.issues.length - 8))

const SUMMARY_TEXT: Record<DeckIssueStatus, (count: number) => string> = {
  banned: (count) => `${count} banned`,
  not_legal: (count) => `${count} not legal`,
  commander_only: (count) => `${count} legal only as the commander`,
  off_colour: (count) => `${count} outside the commander's colour identity`,
  over_limit: (count) => `${count} over the copy limit`,
  restricted: (count) => `${count} restricted over the 1-copy limit`,
}

const summary = computed(() => {
  const counts = new Map<DeckIssueStatus, number>()
  for (const issue of props.legality.issues) {
    counts.set(issue.status, (counts.get(issue.status) ?? 0) + 1)
  }
  return DECK_ISSUE_STATUSES.filter((status) => counts.has(status))
    .map((status) => SUMMARY_TEXT[status](counts.get(status)!))
    .join(', ')
})

const ISSUE_CHIP_CLASSES: Record<DeckIssueStatus, string> = {
  banned: 'bg-red-500/15 text-red-700 dark:text-red-400',
  not_legal: 'bg-muted text-muted-foreground',
  commander_only: 'bg-amber-500/15 text-amber-700 dark:text-amber-400',
  off_colour: 'bg-amber-500/15 text-amber-700 dark:text-amber-400',
  over_limit: 'bg-amber-500/15 text-amber-700 dark:text-amber-400',
  restricted: 'bg-amber-500/15 text-amber-700 dark:text-amber-400',
}

/** Copy counts only help where the breach is about how many you run. */
function issueLabel(issue: DeckLegalityIssue): string {
  return issue.status === 'restricted' || issue.status === 'over_limit'
    ? `${deckIssueLabel(issue.status)} · ${issue.quantity} copies`
    : deckIssueLabel(issue.status)
}
</script>

<template>
  <p v-if="state === 'clean'" class="text-muted-foreground flex items-center gap-1.5 text-sm">
    <CircleCheck
      class="size-4 shrink-0 text-emerald-600 dark:text-emerald-400"
      aria-hidden="true"
    />
    No {{ legality.format_label }} legality issues
  </p>

  <div
    v-else
    class="flex items-start gap-2 rounded-lg border p-3 text-sm"
    :class="
      state === 'error' ? 'border-red-500/40 bg-red-500/10' : 'border-amber-500/40 bg-amber-500/10'
    "
  >
    <TriangleAlert
      class="mt-0.5 size-4 shrink-0"
      :class="
        state === 'error' ? 'text-red-600 dark:text-red-400' : 'text-amber-600 dark:text-amber-400'
      "
      aria-hidden="true"
    />
    <div class="min-w-0 flex-1">
      <p class="font-semibold">
        {{
          state === 'error'
            ? `Not legal in ${legality.format_label}`
            : `${legality.format_label} deck in progress`
        }}
      </p>
      <p v-if="summary" class="text-muted-foreground mt-0.5">{{ summary }}</p>
      <ul v-if="violations.length" class="text-muted-foreground mt-0.5 space-y-0.5">
        <li v-for="(violation, index) in violations" :key="`${violation.rule}-${index}`">
          {{ violation.message }}
        </li>
      </ul>
      <ul v-if="visibleIssues.length" class="mt-2 space-y-1.5">
        <li
          v-for="issue in visibleIssues"
          :key="issue.card_id"
          class="flex flex-wrap items-center gap-1.5"
        >
          <span class="min-w-0 break-words">{{ issue.name }}</span>
          <span
            class="inline-flex shrink-0 items-center rounded-md px-1.5 py-0.5 text-xs font-medium"
            :class="ISSUE_CHIP_CLASSES[issue.status]"
          >
            {{ issueLabel(issue) }}
          </span>
        </li>
      </ul>
      <p v-if="hiddenIssueCount" class="text-muted-foreground mt-1.5">
        …and {{ hiddenIssueCount }} more
      </p>
    </div>
  </div>
</template>
