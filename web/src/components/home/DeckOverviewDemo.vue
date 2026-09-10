<script setup lang="ts">
import { BookCopy, Sparkles } from '@lucide/vue'

// Presentational-only mock of a deck page's collapsed overview strip (glance chips + the
// card-roles bars), for the homepage's decorative feature rows. Every value is illustrative
// and hard-coded — no props, no queries, nothing focusable; a sibling of DemoCardTile.vue.

// The glance chips, worded like `lib/deckOverview.ts`'s but frozen as strings here.
const glances = [
  { label: 'Legal in Commander', class: 'bg-success/15 text-success' },
  { label: 'Bracket 3 · Upgraded', class: 'bg-muted text-muted-foreground' },
  { label: '38 lands · 38%', class: 'bg-muted text-muted-foreground' },
  { label: '2 combos', class: 'bg-muted text-muted-foreground' },
  { label: 'Mana base short on U', class: 'bg-warning/15 text-warning' },
  { label: 'Cheaper by $41.20', class: 'bg-muted text-muted-foreground' },
]

// Role counts, drawn as a share of the biggest one so the bars read as a comparison.
const roles = [
  { label: 'Ramp', count: 11, colour: 'var(--chart-1)' },
  { label: 'Draw', count: 12, colour: 'var(--chart-2)' },
  { label: 'Removal', count: 9, colour: 'var(--chart-3)' },
  { label: 'Wipes', count: 4, colour: 'var(--chart-4)' },
]

const roleMax = Math.max(...roles.map((role) => role.count))

const handCards = [0, 1, 2, 3, 4, 5, 6]
</script>

<template>
  <!-- Decorative mock UI — illustrative values, not real market data. -->
  <div class="flex items-center justify-between gap-2">
    <span class="flex min-w-0 items-center gap-1.5 text-sm font-semibold">
      <BookCopy class="size-4 shrink-0" aria-hidden="true" />
      <span class="truncate">Atraxa, Praetors' Voice</span>
    </span>
    <span class="text-muted-foreground shrink-0 text-[10px] tabular-nums">
      Commander · 100 cards · $412.60
    </span>
  </div>

  <div class="mt-3 flex flex-wrap gap-1.5">
    <span
      v-for="glance in glances"
      :key="glance.label"
      class="rounded-full px-2 py-0.5 text-[10px] font-medium"
      :class="glance.class"
    >
      {{ glance.label }}
    </span>
  </div>

  <div class="mt-4 border-t pt-3">
    <div class="flex items-center gap-1.5 text-xs font-semibold">
      <Sparkles class="size-3.5" aria-hidden="true" />
      Card roles
    </div>
    <div class="mt-2.5 space-y-2">
      <div v-for="role in roles" :key="role.label" class="flex items-center gap-2.5">
        <span class="text-muted-foreground w-14 shrink-0 text-xs">{{ role.label }}</span>
        <span class="bg-muted h-1.5 min-w-0 flex-1 overflow-hidden rounded-full">
          <span
            class="block h-full rounded-full"
            :style="{ width: `${(role.count / roleMax) * 100}%`, background: role.colour }"
          ></span>
        </span>
        <span class="w-5 shrink-0 text-right text-xs tabular-nums">{{ role.count }}</span>
      </div>
    </div>
  </div>

  <div class="mt-4 border-t pt-3">
    <div class="text-muted-foreground text-[10px]">Test hand · seed #4821</div>
    <div class="mt-2 flex gap-1.5">
      <div
        v-for="card in handCards"
        :key="card"
        class="bg-muted h-8 w-6 shrink-0 overflow-hidden rounded border"
      >
        <div class="from-primary/25 via-primary/10 h-2/5 bg-gradient-to-br to-transparent"></div>
      </div>
    </div>
  </div>
</template>
