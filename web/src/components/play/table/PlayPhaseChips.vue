<script setup lang="ts">
import { computed, ref } from 'vue'
import { ChevronDown } from '@lucide/vue'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { PLAY_PHASES } from '@/lib/playTable'
import type { PlayPhase } from '@/lib/api/play'

// Where the turn is, as a segmented pointer.
//
// Seven chips is a comfortable strip on a laptop and most of a phone's width on its own, so on
// a phone it **collapses to the phase it is currently on** and expands on a tap. That is the
// honest trade: the one thing you always need to see is where we are, and the six you rarely
// tap can cost a tap to reach. Expanded, the row scrolls sideways rather than wrapping — a
// wrapping phase row was half the reason the phone turn bar was three rows tall.
//
// Nothing is enforced by phase (this is paper Magic); the chips exist to tell four people who
// are not in the same room where the turn is, which is why only the active seat may move it.
const table = usePlayTableContext()

const expanded = ref(false)
const turn = computed(() => table.store.turn)
const current = computed(
  () => PLAY_PHASES.find((item) => item.phase === turn.value?.phase) ?? PLAY_PHASES[0],
)
/** Collapsed only on a phone, and only until it is opened. */
const collapsed = computed(() => table.compact.value && !expanded.value)

function setPhase(phase: PlayPhase) {
  if (!table.store.isMyTurn || !table.canAct.value) return
  table.send({ type: 'set_phase', phase })
  if (table.compact.value) expanded.value = false
}
</script>

<template>
  <div class="flex min-w-0 items-center gap-0.5" role="group" aria-label="Turn phase">
    <button
      v-if="collapsed"
      type="button"
      class="bg-primary/15 text-primary flex shrink-0 items-center gap-0.5 rounded px-1.5 py-0.5 text-[0.7rem] font-medium"
      :aria-expanded="false"
      aria-label="Turn phase — show all phases"
      @click="expanded = true"
    >
      {{ current.label }}
      <ChevronDown class="size-3" aria-hidden="true" />
    </button>
    <div v-else class="flex min-w-0 gap-0.5 overflow-x-auto">
      <button
        v-for="item in PLAY_PHASES"
        :key="item.phase"
        type="button"
        class="rounded px-1.5 py-0.5 text-[0.7rem] whitespace-nowrap transition-colors"
        :class="
          turn?.phase === item.phase
            ? 'bg-primary text-primary-foreground'
            : 'text-muted-foreground hover:bg-accent'
        "
        :aria-pressed="turn?.phase === item.phase"
        :disabled="!table.store.isMyTurn || !table.canAct.value"
        @click="setPhase(item.phase)"
      >
        {{ item.label }}
      </button>
    </div>
  </div>
</template>
