<script setup lang="ts">
import { computed } from 'vue'
import { Crown, Handshake } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import LifeSparkline from '@/components/life/LifeSparkline.vue'
import type { LifeSeat } from '@/lib/api'
import { seatColor, type LifeLine } from '@/lib/lifeSeries'

// A finished game, as a scoreboard.
//
// Once a result is recorded the mat has nothing left to do — its whole purpose is tapping — so a
// finished game shows the outcome instead: winner first, everyone's final total, the deck each
// played (linked, since that's how you get to its record), and the shape of their game.
const props = defineProps<{ seats: LifeSeat[]; lines: LifeLine[]; game: string }>()

const RESULT_ORDER: Record<string, number> = { win: 0, draw: 1, loss: 2, none: 3 }

const ranked = computed(() =>
  [...props.seats].sort(
    (a, b) => (RESULT_ORDER[a.result] ?? 9) - (RESULT_ORDER[b.result] ?? 9) || b.life - a.life,
  ),
)

const lineFor = (playerId: number) => props.lines.find((line) => line.playerId === playerId)
</script>

<template>
  <ul class="space-y-2">
    <li
      v-for="seat in ranked"
      :key="seat.id"
      class="bg-card flex items-center gap-4 rounded-xl border p-4"
      :class="seat.result === 'win' ? 'border-success/50 bg-success/5' : ''"
    >
      <span
        class="size-2.5 shrink-0 rounded-full"
        :style="{ backgroundColor: seatColor(seat.position) }"
        aria-hidden="true"
      />
      <div class="min-w-0 flex-1">
        <p class="flex items-center gap-2 font-medium">
          <span class="truncate">{{ seat.name }}</span>
          <Crown
            v-if="seat.result === 'win'"
            class="size-4 shrink-0 text-success"
            aria-label="Winner"
          />
          <Handshake
            v-else-if="seat.result === 'draw'"
            class="text-muted-foreground size-4 shrink-0"
            aria-label="Draw"
          />
        </p>
        <RouterLink
          v-if="seat.deck_id !== null && seat.deck_name"
          :to="`/decks/${game}/${seat.deck_id}`"
          class="text-muted-foreground hover:text-foreground truncate text-sm hover:underline"
        >
          {{ seat.deck_name }}
        </RouterLink>
        <p v-else-if="seat.commander_name" class="text-muted-foreground truncate text-sm">
          {{ seat.commander_name }}
        </p>
        <p v-else class="text-muted-foreground text-sm">No deck linked</p>
      </div>
      <div class="hidden h-8 w-24 shrink-0 opacity-70 sm:block">
        <LifeSparkline :line="lineFor(seat.id)" :position="seat.position" />
      </div>
      <span class="w-12 shrink-0 text-right text-2xl font-semibold tabular-nums">
        {{ seat.life }}
      </span>
    </li>
  </ul>
</template>
