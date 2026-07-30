<script setup lang="ts">
import { RouterLink } from 'vue-router'
import type { LifeDeckRecord } from '@/lib/api'
import { winRateLabel } from '@/lib/lifeSeries'
import { WIN_RATE_MIN_GAMES } from '@/lib/lifeLayout'

// How each deck has actually performed, across the games you've tracked.
//
// The one judgement call here is honesty about sample size: a deck that has won its only game
// is not a 100% deck, and printing that number would be the most misleading thing on the page.
// Below `WIN_RATE_MIN_GAMES` the raw W–L–D is still shown in full — the data isn't hidden — but
// the rate column says why it isn't quoting one.
defineProps<{ records: LifeDeckRecord[]; game: string }>()
</script>

<template>
  <div class="overflow-x-auto">
    <table class="w-full text-sm">
      <thead class="text-muted-foreground border-b">
        <tr>
          <th scope="col" class="py-2 pr-3 text-left font-medium">Deck</th>
          <th scope="col" class="px-3 py-2 text-right font-medium">Games</th>
          <th scope="col" class="px-3 py-2 text-right font-medium">W</th>
          <th scope="col" class="px-3 py-2 text-right font-medium">L</th>
          <th scope="col" class="px-3 py-2 text-right font-medium">D</th>
          <th scope="col" class="py-2 pl-3 text-right font-medium">Win rate</th>
        </tr>
      </thead>
      <tbody class="divide-y">
        <tr v-for="record in records" :key="record.deck_id">
          <td class="py-2 pr-3">
            <RouterLink
              :to="`/decks/${game}/${record.deck_id}`"
              class="font-medium hover:underline"
              >{{ record.deck_name }}</RouterLink
            >
          </td>
          <td class="px-3 py-2 text-right tabular-nums">{{ record.games }}</td>
          <td class="px-3 py-2 text-right tabular-nums">{{ record.wins }}</td>
          <td class="px-3 py-2 text-right tabular-nums">{{ record.losses }}</td>
          <td class="px-3 py-2 text-right tabular-nums">{{ record.draws }}</td>
          <td class="py-2 pl-3 text-right">
            <span v-if="record.games >= WIN_RATE_MIN_GAMES" class="font-medium tabular-nums">
              {{ winRateLabel(record.win_rate) }}
            </span>
            <span
              v-else
              class="text-muted-foreground text-xs"
              :title="`A win rate needs at least ${WIN_RATE_MIN_GAMES} games to mean anything.`"
            >
              too few games
            </span>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
