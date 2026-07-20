<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { TrendingDown, TrendingUp } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import MoverRow from '@/components/collection/MoverRow.vue'
import SetsScopeToggle from '@/components/collection/SetsScopeToggle.vue'
import { useCollectionMoversQuery } from '@/composables/useCollection'
import type { CollectionMover, CollectionSealedMover, MoverWindow } from '@/lib/api'

// The collection landing's "Biggest movers" panel (issues #360/#392/#435): the largest gain
// and loss movements across the cards and sealed products the user owns, from one day through
// all captured history.
// The active window is fetched on demand — only the visible date range is computed server-side
// — so most visitors, who never leave the default, never pay for the other six windows or the
// all-time scan. Switching windows refetches once then caches (switching back is instant), and
// the Singles/Sealed switch stays a pure client-side toggle since both kinds ship in each
// window response. Week is the default: daily moves are often tiny or empty on a young
// collection, while longer windows hide the news.
const props = defineProps<{ game: string }>()
const gameId = toRef(() => props.game)

const activeWindow = ref<MoverWindow>('week')
const showSealed = ref(false)
const query = useCollectionMoversQuery(gameId, activeWindow)
const WINDOW_OPTIONS: { value: MoverWindow; label: string }[] = [
  { value: 'day', label: '1D' },
  { value: 'week', label: '7D' },
  { value: 'month', label: '30D' },
  { value: 'year', label: '1Y' },
  { value: 'two_year', label: '2Y' },
  { value: 'three_year', label: '3Y' },
  { value: 'all_time', label: 'All' },
]

const movers = computed(() => query.data.value)
const activeSeries = computed(() => (showSealed.value ? movers.value?.sealed : movers.value))
const activeList = computed(() => activeSeries.value?.[activeWindow.value])
const gainers = computed(() => activeList.value?.gainers ?? [])
const losers = computed(() => activeList.value?.losers ?? [])
function moverKey(mover: CollectionMover | CollectionSealedMover) {
  return 'product' in mover ? `product:${mover.product.id}` : `card:${mover.card.id}`
}
// A young collection can have day movement before week/month baselines exist, so
// emptiness is judged per-window (both sides empty → one centered message).
const windowEmpty = computed(() => !gainers.value.length && !losers.value.length)

// With no captured price history at all (`as_of` null — a brand-new collection) every
// window is empty, so the whole card renders nothing rather than an empty shell. The
// pending/error states still show so the panel doesn't pop in after load.
const visible = computed(
  () =>
    query.isPending.value ||
    query.isError.value ||
    movers.value?.as_of != null ||
    movers.value?.sealed.as_of != null,
)

// The reference date the movements are measured to, e.g. "Jul 12" — shown subtly next
// to the title so a stale snapshot is legible as such. The 1D list can fall back to the
// previous available snapshot day independently of the longer windows.
const asOfText = computed(() => {
  const asOf =
    activeWindow.value === 'day' ? activeSeries.value?.day_as_of : activeSeries.value?.as_of
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
        <div class="flex max-w-full flex-wrap items-center justify-end gap-2">
          <SetsScopeToggle v-model="showSealed" collected-label="Singles" second-label="Sealed" />
          <div
            class="bg-muted/50 flex max-w-full flex-wrap items-center justify-end gap-1 rounded-lg p-0.5"
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
        <!-- `min-w-0`: these sections are grid items, which default to `min-width: auto` and
             so refuse to shrink below their rows' min-content. On a narrow (mobile) single
             column that let each MoverRow overflow the card, clipping the price column off
             the right edge; allowing the track to shrink lets the row's name truncate instead. -->
        <section class="min-w-0">
          <h3
            class="flex items-center gap-1.5 text-xs font-semibold tracking-wide uppercase text-emerald-700 dark:text-emerald-400"
          >
            <TrendingUp class="size-3.5" aria-hidden="true" />
            Gainers
          </h3>
          <ul v-if="gainers.length" class="mt-2 space-y-1">
            <li v-for="mover in gainers" :key="moverKey(mover)">
              <MoverRow :game="game" :mover="mover" />
            </li>
          </ul>
          <p v-else class="text-muted-foreground mt-2 text-sm">No gainers.</p>
        </section>
        <section class="min-w-0">
          <h3
            class="flex items-center gap-1.5 text-xs font-semibold tracking-wide uppercase text-red-700 dark:text-red-400"
          >
            <TrendingDown class="size-3.5" aria-hidden="true" />
            Losers
          </h3>
          <ul v-if="losers.length" class="mt-2 space-y-1">
            <li v-for="mover in losers" :key="moverKey(mover)">
              <MoverRow :game="game" :mover="mover" />
            </li>
          </ul>
          <p v-else class="text-muted-foreground mt-2 text-sm">No losers.</p>
        </section>
      </div>
    </CardContent>
  </Card>
</template>
