<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { TrendingDown, TrendingUp } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import MoverRow from '@/components/collection/MoverRow.vue'
import { useCollectionMoversQuery } from '@/composables/useCollection'

// The collection landing's "Biggest movers" panel (issue #360): the largest gain and
// loss movements across the cards the user owns, over a day / week / month window. One
// response carries all three windows, so the segmented toggle is purely client-side —
// switching is instant, no refetch. Week is the default: daily moves are often tiny or
// empty on a young collection, monthly hides the news.
const props = defineProps<{ game: string }>()
const gameId = toRef(() => props.game)
const query = useCollectionMoversQuery(gameId)

type MoverWindow = 'day' | 'week' | 'month'
const activeWindow = ref<MoverWindow>('week')
const WINDOW_OPTIONS: { value: MoverWindow; label: string }[] = [
  { value: 'day', label: 'Day' },
  { value: 'week', label: 'Week' },
  { value: 'month', label: 'Month' },
]

const movers = computed(() => query.data.value)
const activeList = computed(() => movers.value?.[activeWindow.value])
const gainers = computed(() => activeList.value?.gainers ?? [])
const losers = computed(() => activeList.value?.losers ?? [])
// A young collection can have day movement before week/month baselines exist, so
// emptiness is judged per-window (both sides empty → one centered message).
const windowEmpty = computed(() => !gainers.value.length && !losers.value.length)

// With no captured price history at all (`as_of` null — a brand-new collection) every
// window is empty, so the whole card renders nothing rather than an empty shell. The
// pending/error states still show so the panel doesn't pop in after load.
const visible = computed(
  () => query.isPending.value || query.isError.value || movers.value?.as_of != null,
)

// The reference date the movements are measured to, e.g. "Jul 12" — shown subtly next
// to the title so a stale snapshot is legible as such.
const asOfText = computed(() => {
  const asOf = movers.value?.as_of
  if (!asOf) return null
  const date = new Date(`${asOf}T00:00:00`)
  if (Number.isNaN(date.getTime())) return asOf
  return new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' }).format(date)
})
</script>

<template>
  <Card v-if="visible" class="mt-6">
    <CardHeader>
      <div class="flex flex-wrap items-center justify-between gap-2">
        <div class="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
          <CardTitle class="text-sm font-semibold">Biggest movers</CardTitle>
          <span v-if="asOfText" class="text-muted-foreground text-xs">as of {{ asOfText }}</span>
        </div>
        <div
          class="bg-muted/50 inline-flex items-center gap-1 rounded-lg p-0.5"
          role="group"
          aria-label="Biggest movers window"
        >
          <Button
            v-for="opt in WINDOW_OPTIONS"
            :key="opt.value"
            type="button"
            :variant="activeWindow === opt.value ? 'secondary' : 'ghost'"
            size="sm"
            class="h-8 px-2.5 text-xs font-medium"
            :aria-pressed="activeWindow === opt.value"
            @click="activeWindow = opt.value"
          >
            {{ opt.label }}
          </Button>
        </div>
      </div>
    </CardHeader>
    <CardContent>
      <!-- Loading: placeholder rows shaped like the loaded grid (Skeleton is
           reduced-motion aware), so the panel doesn't jump when the data lands. -->
      <div
        v-if="query.isPending.value"
        class="grid gap-x-6 gap-y-4 sm:grid-cols-2"
        aria-hidden="true"
      >
        <div v-for="side in 2" :key="side" class="space-y-2">
          <Skeleton class="h-4 w-24" />
          <Skeleton v-for="row in 3" :key="row" class="h-14 w-full" />
        </div>
      </div>
      <p v-else-if="query.isError.value" class="text-muted-foreground py-12 text-sm">
        Couldn't load movers.
      </p>
      <p v-else-if="windowEmpty" class="text-muted-foreground py-10 text-center text-sm">
        Not enough price history yet for this window.
      </p>
      <div v-else class="grid gap-x-6 gap-y-4 sm:grid-cols-2">
        <section>
          <h3
            class="flex items-center gap-1.5 text-xs font-semibold tracking-wide uppercase text-emerald-700 dark:text-emerald-400"
          >
            <TrendingUp class="size-3.5" aria-hidden="true" />
            Gainers
          </h3>
          <ul v-if="gainers.length" class="mt-2 space-y-1">
            <li v-for="mover in gainers" :key="mover.card.id">
              <MoverRow :game="game" :mover="mover" />
            </li>
          </ul>
          <p v-else class="text-muted-foreground mt-2 text-sm">No gainers.</p>
        </section>
        <section>
          <h3
            class="flex items-center gap-1.5 text-xs font-semibold tracking-wide uppercase text-red-700 dark:text-red-400"
          >
            <TrendingDown class="size-3.5" aria-hidden="true" />
            Losers
          </h3>
          <ul v-if="losers.length" class="mt-2 space-y-1">
            <li v-for="mover in losers" :key="mover.card.id">
              <MoverRow :game="game" :mover="mover" />
            </li>
          </ul>
          <p v-else class="text-muted-foreground mt-2 text-sm">No losers.</p>
        </section>
      </div>
    </CardContent>
  </Card>
</template>
