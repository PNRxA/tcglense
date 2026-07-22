<script setup lang="ts">
import { computed } from 'vue'
import { CircleCheck, TriangleAlert } from '@lucide/vue'
import {
  legalityLabel,
  type DeckIssueStatus,
  type DeckLegality,
  type DeckLegalityIssue,
} from '@/lib/legality'

const props = defineProps<{
  legality: DeckLegality
}>()

const visibleIssues = computed(() => props.legality.issues.slice(0, 8))
const hiddenIssueCount = computed(() => Math.max(0, props.legality.issues.length - 8))

const summary = computed(() => {
  const counts: Record<DeckIssueStatus, number> = {
    banned: 0,
    not_legal: 0,
    restricted: 0,
  }
  for (const issue of props.legality.issues) counts[issue.status] += 1

  return [
    counts.banned ? `${counts.banned} banned` : null,
    counts.not_legal ? `${counts.not_legal} not legal` : null,
    counts.restricted ? `${counts.restricted} restricted over the 1-copy limit` : null,
  ]
    .filter((part): part is string => part != null)
    .join(', ')
})

const ISSUE_CHIP_CLASSES: Record<DeckIssueStatus, string> = {
  banned: 'bg-red-500/15 text-red-700 dark:text-red-400',
  not_legal: 'bg-muted text-muted-foreground',
  restricted: 'bg-amber-500/15 text-amber-700 dark:text-amber-400',
}

function issueLabel(issue: DeckLegalityIssue): string {
  return issue.status === 'restricted'
    ? `${legalityLabel(issue.status)} · ${issue.quantity} copies`
    : legalityLabel(issue.status)
}
</script>

<template>
  <p
    v-if="legality.issues.length === 0"
    class="text-muted-foreground flex items-center gap-1.5 text-sm"
  >
    <CircleCheck
      class="size-4 shrink-0 text-emerald-600 dark:text-emerald-400"
      aria-hidden="true"
    />
    No {{ legality.formatLabel }} legality issues
  </p>

  <div
    v-else
    class="flex items-start gap-2 rounded-lg border border-red-500/40 bg-red-500/10 p-3 text-sm"
  >
    <TriangleAlert
      class="mt-0.5 size-4 shrink-0 text-red-600 dark:text-red-400"
      aria-hidden="true"
    />
    <div class="min-w-0 flex-1">
      <p class="font-semibold">Not legal in {{ legality.formatLabel }}</p>
      <p class="text-muted-foreground mt-0.5">{{ summary }}</p>
      <ul class="mt-2 space-y-1.5">
        <li
          v-for="issue in visibleIssues"
          :key="issue.cardId"
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
