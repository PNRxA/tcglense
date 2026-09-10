<script setup lang="ts">
import { HeartPulse, Skull, Swords } from '@lucide/vue'

// Presentational-only mock of the life counter's table view, for the homepage's decorative
// feature rows. Every value is illustrative and hard-coded — no props, no queries, nothing
// focusable; a sibling of DemoCardTile.vue and DeckOverviewDemo.vue.

// One seat each: the total, its accent (a low total reads warning, as the real mat does), the
// counter line under it, and an optional delta chip suggesting the last tap.
interface DemoSeat {
  name: string
  life: number
  /** Accent for the total — the low seat reads warning, as the real mat's does. */
  lifeClass?: string
  note: string
  /** The note names the deck this player brought, rather than a counter. */
  deck?: boolean
  icon?: typeof Skull
  delta?: string
  deltaClass?: string
}

const seats: DemoSeat[] = [
  { name: 'Sam', life: 40, note: 'Deck · Atraxa', deck: true },
  { name: 'Priya', life: 33, note: 'Poison 3', icon: Skull },
  {
    name: 'Jo',
    life: 28,
    note: 'Energy 2',
    delta: '+4',
    deltaClass: 'bg-success/15 text-success',
  },
  {
    name: 'Alex',
    life: 12,
    lifeClass: 'text-warning',
    note: 'Cmdr dmg 15 from Priya',
    icon: Swords,
    delta: '−3',
    deltaClass: 'bg-destructive/15 text-destructive',
  },
]
</script>

<template>
  <!-- Decorative mock UI — illustrative values, not real market data. -->
  <div class="flex items-center justify-between gap-2">
    <span class="flex min-w-0 items-center gap-1.5 text-sm font-semibold">
      <HeartPulse class="size-4 shrink-0" aria-hidden="true" />
      <span class="truncate">Commander · 4 players</span>
    </span>
    <div class="flex shrink-0 items-center gap-1">
      <span class="text-muted-foreground rounded-full border px-2 py-0.5 text-[10px]">Poison</span>
      <span class="text-muted-foreground rounded-full border px-2 py-0.5 text-[10px]">
        Cmdr dmg
      </span>
    </div>
  </div>

  <div class="mt-3 grid grid-cols-2 gap-2">
    <div v-for="seat in seats" :key="seat.name" class="rounded-lg border p-3">
      <div class="flex items-center justify-between gap-2">
        <span class="truncate text-xs font-medium">{{ seat.name }}</span>
        <span
          v-if="seat.delta"
          class="shrink-0 rounded-full px-1.5 text-[10px] tabular-nums"
          :class="seat.deltaClass"
        >
          {{ seat.delta }}
        </span>
      </div>
      <div class="mt-1 text-2xl font-semibold tabular-nums" :class="seat.lifeClass">
        {{ seat.life }}
      </div>
      <div class="text-muted-foreground mt-1 flex items-center gap-1 text-[10px]">
        <component :is="seat.icon" v-if="seat.icon" class="size-3 shrink-0" aria-hidden="true" />
        <span class="truncate" :class="seat.deck ? 'italic' : ''">{{ seat.note }}</span>
      </div>
      <div v-if="!seat.deck" class="bg-foreground/15 mt-2 h-1.5 w-16 rounded-full"></div>
    </div>
  </div>

  <div class="text-muted-foreground mt-3 flex items-center justify-between gap-2 text-[10px]">
    <span class="truncate">Turn 7 · history kept for undo</span>
    <span class="shrink-0 rounded-full border px-2 py-0.5 tabular-nums">Win record 6–2</span>
  </div>
</template>
