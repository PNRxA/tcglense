<script setup lang="ts">
import { computed } from 'vue'
import { ArrowDown, Crosshair, Zap } from '@lucide/vue'
import { resolveOrderLabel } from '@/lib/stack/hints'
import { describeTarget } from '@/lib/stack/targets'
import type { StackObject, StackState } from '@/lib/stack/types'

// The stack itself, drawn top-down so the object that resolves next is at the top — the
// mental picture the rules describe ("on top of the stack") rather than the array's order.
const props = defineProps<{ state: StackState }>()

const KIND_LABELS: Record<StackObject['kind'], string> = {
  spell: 'Spell',
  activated: 'Activated ability',
  triggered: 'Triggered ability',
  copy: 'Copy of a spell',
}

const rows = computed(() => {
  const total = props.state.stack.length
  return [...props.state.stack]
    .map((object, position) => ({
      object,
      order: resolveOrderLabel(position, total),
      target: object.target ? describeTarget(props.state, object.target) : null,
      controller: props.state.players[object.controller].name,
    }))
    .reverse()
})
</script>

<template>
  <section class="bg-card rounded-xl border p-4 shadow-sm" aria-labelledby="stack-heading">
    <div class="mb-3 flex items-center justify-between gap-2">
      <h2 id="stack-heading" class="text-sm font-semibold">The stack</h2>
      <span class="text-muted-foreground text-xs">
        {{ state.stack.length === 0 ? 'Empty' : `${state.stack.length} waiting` }}
      </span>
    </div>

    <p
      v-if="rows.length === 0"
      class="text-muted-foreground rounded-lg border border-dashed px-3 py-6 text-center text-sm"
    >
      Nothing on the stack. Cast a spell to put something here.
    </p>

    <ol v-else class="space-y-2" data-testid="stack-objects">
      <li
        v-for="(row, index) in rows"
        :key="row.object.id"
        class="rounded-lg border p-3"
        :class="index === 0 ? 'border-primary/50 bg-primary/5' : 'bg-background'"
      >
        <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
          <span
            class="rounded-full px-2 py-0.5 text-[0.65rem] font-medium tracking-wide uppercase"
            :class="
              row.object.kind === 'spell' || row.object.kind === 'copy'
                ? 'bg-muted text-muted-foreground'
                : 'bg-warning/15 text-warning'
            "
          >
            {{ KIND_LABELS[row.object.kind] }}
          </span>
          <span
            v-if="row.object.splitSecond"
            class="bg-destructive/15 text-destructive inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[0.65rem] font-medium"
          >
            <Zap class="size-3" aria-hidden="true" /> Split second
          </span>
          <span
            class="text-muted-foreground ml-auto text-xs"
            :class="index === 0 ? 'text-primary font-medium' : ''"
          >
            {{ row.order }}
          </span>
        </div>
        <p class="mt-1.5 font-medium">
          {{ row.object.name }}
          <span class="text-muted-foreground text-xs font-normal">· {{ row.controller }}</span>
        </p>
        <p v-if="row.target" class="text-muted-foreground mt-0.5 flex items-center gap-1 text-xs">
          <Crosshair class="size-3 shrink-0" aria-hidden="true" /> targeting {{ row.target }}
        </p>
        <p v-if="row.object.text" class="text-muted-foreground mt-1 text-xs leading-relaxed">
          {{ row.object.text }}
        </p>
        <p
          v-if="index < rows.length - 1"
          class="text-muted-foreground mt-2 flex items-center gap-1 text-[0.65rem]"
        >
          <ArrowDown class="size-3" aria-hidden="true" /> below it
        </p>
      </li>
    </ol>
  </section>
</template>
