<script setup lang="ts">
import { computed } from 'vue'
import { Crosshair, Hand, Heart, RotateCw } from '@lucide/vue'
import { statsLabel } from '@/lib/stack/resolve'
import type { PlayerId, StackState } from '@/lib/stack/types'

// One player's side of the table: life, whose turn it is, who holds priority, and each
// permanent with the state that matters to the stack — power/toughness with any pump,
// marked damage, tapped, and whether something on the stack is aiming at it.
const props = defineProps<{ state: StackState; player: PlayerId }>()

const me = computed(() => props.state.players[props.player])
const isActive = computed(() => props.state.activePlayer === props.player)
const hasPriority = computed(() => props.state.priority === props.player && !props.state.loser)
const lost = computed(() => props.state.loser === props.player)

const permanents = computed(() =>
  props.state.battlefield
    .filter((p) => p.controller === props.player)
    .map((p) => ({
      permanent: p,
      stats: statsLabel(p),
      pumped: p.pumpPower !== 0 || p.pumpToughness !== 0,
      targetedBy: props.state.stack
        .filter((o) => o.target?.kind === 'permanent' && o.target.id === p.id)
        .map((o) => o.name),
    })),
)

const targetedBy = computed(() =>
  props.state.stack
    .filter((o) => o.target?.kind === 'player' && o.target.id === props.player)
    .map((o) => o.name),
)
</script>

<template>
  <section
    class="rounded-xl border p-4 transition-colors"
    :class="[hasPriority ? 'border-primary/60 bg-primary/5' : 'bg-card', lost ? 'opacity-60' : '']"
    :aria-label="`${me.name}'s board`"
  >
    <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
      <h2 class="font-semibold">{{ me.name }}</h2>
      <span
        v-if="isActive"
        class="bg-muted text-muted-foreground rounded-full px-2 py-0.5 text-[0.65rem] font-medium tracking-wide uppercase"
      >
        Active player
      </span>
      <span
        v-if="hasPriority"
        class="bg-primary/15 text-primary inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[0.65rem] font-medium tracking-wide uppercase"
      >
        <Hand class="size-3" aria-hidden="true" /> Has priority
      </span>
      <span
        v-if="lost"
        class="bg-destructive/15 text-destructive rounded-full px-2 py-0.5 text-[0.65rem] font-medium tracking-wide uppercase"
      >
        Lost
      </span>
      <span class="ml-auto inline-flex items-center gap-1 text-lg font-semibold tabular-nums">
        <Heart class="text-destructive size-4" aria-hidden="true" />
        {{ me.life }}
      </span>
    </div>
    <p v-if="targetedBy.length" class="text-muted-foreground mt-1 flex items-center gap-1 text-xs">
      <Crosshair class="size-3" aria-hidden="true" /> targeted by {{ targetedBy.join(', ') }}
    </p>

    <p v-if="permanents.length === 0" class="text-muted-foreground mt-3 text-xs">No permanents.</p>
    <ul v-else class="mt-3 flex flex-wrap gap-2">
      <li
        v-for="row in permanents"
        :key="row.permanent.id"
        class="bg-background min-w-36 rounded-lg border px-3 py-2 text-sm"
        :class="row.permanent.tapped ? 'opacity-70' : ''"
      >
        <div class="flex items-center gap-2">
          <span class="font-medium">{{ row.permanent.name }}</span>
          <span
            v-if="row.stats"
            class="ml-auto font-semibold tabular-nums"
            :class="row.pumped ? 'text-success' : ''"
          >
            {{ row.stats }}
          </span>
        </div>
        <p class="text-muted-foreground mt-0.5 text-[0.7rem] capitalize">
          {{ row.permanent.type }}
        </p>
        <div class="mt-1 flex flex-wrap gap-1">
          <span
            v-if="row.pumped"
            class="bg-success/15 text-success rounded-full px-1.5 py-0.5 text-[0.65rem] font-medium"
          >
            +{{ row.permanent.pumpPower }}/+{{ row.permanent.pumpToughness }} until end of turn
          </span>
          <span
            v-if="row.permanent.damage > 0"
            class="bg-destructive/15 text-destructive rounded-full px-1.5 py-0.5 text-[0.65rem] font-medium"
          >
            {{ row.permanent.damage }} damage
          </span>
          <span
            v-if="row.permanent.tapped"
            class="bg-muted text-muted-foreground inline-flex items-center gap-1 rounded-full px-1.5 py-0.5 text-[0.65rem] font-medium"
          >
            <RotateCw class="size-3" aria-hidden="true" /> Tapped
          </span>
          <span
            v-for="name in row.targetedBy"
            :key="name"
            class="bg-warning/15 text-warning inline-flex items-center gap-1 rounded-full px-1.5 py-0.5 text-[0.65rem] font-medium"
          >
            <Crosshair class="size-3" aria-hidden="true" /> {{ name }}
          </span>
        </div>
      </li>
    </ul>
  </section>
</template>
