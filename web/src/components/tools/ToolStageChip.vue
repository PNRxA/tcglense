<script setup lang="ts">
import { TOOL_STAGE_LABELS, type ToolStage } from '@/lib/tools'

// The maturity tag beside a tool's name — "Alpha" today. One component so the tools hub, a
// game's index, both navs and the tool's own page all spell it the same way and change together.
//
// `warning` is the design system's "unfinished" token (docs/design-system.md), and this is the
// chip idiom (`bg-<token>/15 text-<token>`), never a palette literal. The `title` carries the
// long form for hover and screen readers, so the tag doesn't need a sentence beside it on
// surfaces as tight as a nav row.
defineProps<{ stage: ToolStage }>()

const STAGE_TITLES: Record<ToolStage, string> = {
  alpha: 'Alpha: usable, but still settling — expect rough edges and changes.',
}
</script>

<template>
  <span
    class="bg-warning/15 text-warning inline-flex shrink-0 items-center rounded px-1.5 py-0.5 text-[0.65rem] font-semibold tracking-wide uppercase"
    :title="STAGE_TITLES[stage]"
  >
    {{ TOOL_STAGE_LABELS[stage] }}
  </span>
</template>
