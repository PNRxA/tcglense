<script setup lang="ts">
import { computed, ref, useId, watch } from 'vue'
import { Keyboard, Sparkles, Undo2 } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import CardImage from '@/components/cards/CardImage.vue'
import { useCurrency } from '@/composables/useCurrency'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import { finishUsdPrice } from '@/lib/cardPrice'
import { holdingDelta, holdingDeltaIsFoil, holdingDeltaLabel } from '@/lib/holdingDelta'
import type { SessionEntry } from '@/composables/useScanSession'

// The running tally of cards added this session, newest first — quick reassurance that
// scans are landing, and a one-tap undo for the inevitable misread. Rows cover cards added
// by name from the page's quick-add box too, so the tally matches what the session actually
// wrote to the collection.
const props = defineProps<{
  game: string
  entries: SessionEntry[]
  disabled: boolean
}>()

const emit = defineEmits<{ undo: [number] }>()
const listId = useId()
const expanded = ref(false)

// Same navigation contract as CardTile and the deck list: a plain left-click opens the
// shared detail modal over the scan page — so the camera, the tentative match, and the log
// all survive a look at the full card — while the href stays the real card page for
// modifier/middle clicks and new-tab.
const { hrefFor, onActivate, warm } = useDetailModalLink()

const money = useCurrency()

// Each visible row, pre-shaped: what the write changed (which makes the finish the copy
// landed on obvious at a glance — the scanner picks foil off a printed ★ and can be wrong),
// and the card's price for exactly that finish.
const rows = computed(() =>
  (expanded.value ? props.entries : props.entries.slice(0, 3)).map((entry) => {
    const delta = holdingDelta(entry.previous, entry)
    const foil = holdingDeltaIsFoil(delta)
    const price = finishUsdPrice(entry.card.prices, foil)
    return {
      entry,
      foil,
      deltaLabel: holdingDeltaLabel(delta),
      price: price ? { ...price, text: money.formatUsd(price.amount) } : null,
    }
  }),
)

watch(
  () => props.entries.length,
  (length) => {
    if (length <= 3) expanded.value = false
  },
)
</script>

<template>
  <ul :id="listId" class="divide-border divide-y">
    <li
      v-for="(row, index) in rows"
      :key="row.entry.id"
      class="flex min-w-0 items-center gap-3 py-2"
    >
      <a
        :href="hrefFor('card', game, row.entry.card.id)"
        class="group focus-visible:ring-ring flex min-w-0 flex-1 items-center gap-3 rounded-md focus-visible:ring-2 focus-visible:outline-none"
        @click="onActivate($event, 'card', game, row.entry.card.id)"
        @pointerenter="warm('card')"
        @focusin="warm('card')"
      >
        <CardImage
          :game="game"
          :id="row.entry.card.id"
          :name="row.entry.card.name"
          :has-image="row.entry.card.has_image"
          size="small"
          class="w-9 shrink-0"
        />
        <div class="min-w-0 flex-1 space-y-0.5">
          <p class="flex min-w-0 items-baseline gap-1.5">
            <span class="truncate text-sm font-medium group-hover:underline">
              {{ row.entry.card.name }}
            </span>
            <!-- A card that arrived through the add-by-name box rather than the camera. The
              row is otherwise identical — it is the same write to the same collection — so
              this is a quiet mark, not a second layout. -->
            <span v-if="row.entry.source === 'manual'" class="shrink-0 self-center">
              <Keyboard class="text-muted-foreground size-3.5" aria-hidden="true" />
              <span class="sr-only">added by name</span>
            </span>
            <!-- The card's value, for the finish this row added: what a scanning session is
              usually adding up as it goes. -->
            <span
              v-if="row.price?.text"
              class="text-muted-foreground ml-auto shrink-0 text-xs tabular-nums"
            >
              {{ row.price.text
              }}<span v-if="row.price.foil" class="ml-1 uppercase opacity-70">foil</span>
            </span>
          </p>
          <p class="flex min-w-0 items-center gap-1.5 text-xs">
            <!-- What this row changed, colour-coded by finish — "did that go to foil or
              regular?" is the question a scanning session asks most often, and the resulting
              counts beside it don't answer it. Never truncated: it leads the line. -->
            <span
              v-if="row.deltaLabel"
              class="inline-flex shrink-0 items-center gap-0.5 rounded-full px-1.5 py-0.5 text-[0.6875rem] font-medium tabular-nums ring-1"
              :class="
                row.foil
                  ? 'bg-amber-500/10 text-amber-700 ring-amber-500/30 dark:text-amber-400'
                  : 'bg-primary/10 text-primary ring-primary/20'
              "
            >
              <Sparkles v-if="row.foil" class="size-3" aria-hidden="true" />
              {{ row.deltaLabel }}
            </span>
            <span class="text-muted-foreground min-w-0 truncate">
              {{ row.entry.card.set_code.toUpperCase() }} · #{{ row.entry.card.collector_number }} ·
              <span class="tabular-nums">
                Now {{ row.entry.quantity }} regular<template v-if="row.entry.foil_quantity">
                  · {{ row.entry.foil_quantity }} foil</template
                >
              </span>
            </span>
          </p>
        </div>
      </a>
      <Button
        variant="ghost"
        size="sm"
        class="text-muted-foreground min-h-11 shrink-0 lg:min-h-8"
        :disabled="disabled"
        :aria-label="`Undo adding ${row.entry.card.name}`"
        @click="emit('undo', index)"
      >
        <Undo2 class="size-4" aria-hidden="true" />
        Undo
      </Button>
    </li>
  </ul>

  <Button
    v-if="entries.length > 3"
    variant="ghost"
    size="sm"
    class="mt-1 min-h-11 w-full lg:min-h-8"
    :aria-expanded="expanded"
    :aria-controls="listId"
    @click="expanded = !expanded"
  >
    {{ expanded ? 'Show less' : `View all (${entries.length})` }}
  </Button>
</template>
