<script setup lang="ts">
import { computed, toRef } from 'vue'
import { Swords } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { useLifeDeckRecordsQuery } from '@/composables/useLifeCounter'
import { WIN_RATE_MIN_GAMES } from '@/lib/lifeLayout'
import { winRateLabel } from '@/lib/lifeSeries'
import { lifeDeckStatsPath } from '@/lib/tools'

// A deck's own match record, on the deck page.
//
// This is the payoff for linking a seat to a deck in the life counter: the deck you're looking at
// tells you how it has actually done. It asks the API for *this deck only* (`?deck_id=`) rather
// than pulling every deck's record to show one line.
//
// It renders **nothing** when the deck has never been played — a deck page shouldn't grow an
// empty "0–0" widget for a feature its owner may not use.
const props = defineProps<{ game: string; deckId: number }>()

const deckId = toRef(props, 'deckId')
const { data } = useLifeDeckRecordsQuery(toRef(props, 'game'), deckId)
const record = computed(() => data.value?.data?.find((row) => row.deck_id === props.deckId))

const rate = computed(() => {
  const row = record.value
  if (!row || row.games < WIN_RATE_MIN_GAMES) return null
  return winRateLabel(row.win_rate)
})
</script>

<template>
  <p
    v-if="record && record.games > 0"
    class="text-muted-foreground flex items-center gap-1.5 text-sm"
  >
    <Swords class="size-3.5 shrink-0" aria-hidden="true" />
    <span class="tabular-nums">
      {{ record.wins }}–{{ record.losses
      }}<template v-if="record.draws > 0">–{{ record.draws }}</template> in
      {{ record.games }} tracked {{ record.games === 1 ? 'game' : 'games' }}
      <template v-if="rate"> · {{ rate }} win rate</template>
    </span>
    <span aria-hidden="true">·</span>
    <RouterLink
      :to="lifeDeckStatsPath(game)"
      class="hover:text-foreground shrink-0 hover:underline"
    >
      All records
    </RouterLink>
  </p>
</template>
