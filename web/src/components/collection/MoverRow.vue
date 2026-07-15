<script lang="ts">
// Warm the shared card-detail dialog chunk on the first hover/focus of ANY row (module
// flag → once per session), mirroring CardTile, so the click that opens ?card= finds
// the chunk already fetched.
let dialogWarmed = false
</script>

<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import type { CollectionMover } from '@/lib/api'
import CardImage from '@/components/cards/CardImage.vue'
import { loadCardDetailDialog } from '@/components/cards/detailDialogLoader'
import { useCurrency } from '@/composables/useCurrency'

// One row of the collection landing's "Biggest movers" panel: card thumbnail + name/set
// on the left, the holding's signed value change (with a % chip and the current holding
// value) right-aligned. The whole row is a link to the card — a plain left-click opens
// the shared `?card=` detail modal (CardTile's idiom) so the landing keeps its state,
// while the href stays the real card page for modifier/middle clicks and new tabs.
const props = defineProps<{ game: string; mover: CollectionMover }>()
const money = useCurrency()

const route = useRoute()
const router = useRouter()
const to = computed(() => `/cards/${props.game}/cards/${props.mover.card.id}`)
const href = computed(() => router.resolve(to.value).href)
function onClick(event: MouseEvent) {
  if (event.defaultPrevented) return
  // Let the browser handle anything that isn't a plain left-click.
  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
    return
  }
  event.preventDefault()
  void router.push({ query: { ...route.query, card: props.mover.card.id } })
}

// Fire-and-forget prefetch of the detail-dialog chunk on first hover/focus.
function warmCardDetailDialog() {
  if (dialogWarmed) return
  dialogWarmed = true
  void loadCardDetailDialog()
}

// Gain/loss is read off the change itself (gainers are positive, losers negative).
// `change_usd` is a SIGNED decimal string, so the sign is stripped before formatUsd
// (which would otherwise render "$-3.50") and re-applied as a real minus (U+2212),
// whose glyph width matches the plus.
const change = computed(() => Number(props.mover.change_usd))
const isGain = computed(() => change.value >= 0)
const changeText = computed(() => {
  if (!Number.isFinite(change.value)) return props.mover.change_usd
  return `${isGain.value ? '+' : '−'}${money.formatUsd(String(Math.abs(change.value)))}`
})
const pctText = computed(() => {
  const pct = props.mover.change_pct
  if (pct == null) return null
  return `${pct >= 0 ? '+' : '−'}${Math.abs(pct).toFixed(1)}%`
})
const valueNow = computed(() => money.formatUsd(props.mover.value_now))

// Light-mode uses the -700 greens/reds (not -600): -600 falls under the 4.5:1 WCAG AA
// contrast threshold on the white card, matching the -700 the chips already use.
const deltaClass = computed(() =>
  isGain.value ? 'text-emerald-700 dark:text-emerald-400' : 'text-red-700 dark:text-red-400',
)
const chipClass = computed(() =>
  isGain.value
    ? 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-400'
    : 'bg-red-500/15 text-red-700 dark:text-red-400',
)
</script>

<template>
  <a
    :href="href"
    class="group hover:bg-muted/50 -mx-2 flex items-center gap-3 rounded-md px-2 py-1.5"
    @click="onClick"
    @pointerenter="warmCardDetailDialog"
    @focusin="warmCardDetailDialog"
  >
    <CardImage
      :game="game"
      :id="mover.card.id"
      :name="mover.card.name"
      :has-image="mover.card.has_image"
      size="small"
      class="w-10 shrink-0"
    />
    <div class="min-w-0 flex-1">
      <p class="truncate text-sm font-medium group-hover:underline" :title="mover.card.name">
        {{ mover.card.name }}
      </p>
      <p class="text-muted-foreground truncate text-xs">
        {{ mover.card.set_code.toUpperCase() }} · #{{ mover.card.collector_number }}
      </p>
    </div>
    <div class="shrink-0 text-right">
      <p class="text-sm font-semibold tabular-nums" :class="deltaClass">{{ changeText }}</p>
      <p class="mt-0.5 flex items-center justify-end gap-1.5">
        <span
          v-if="pctText"
          class="rounded-md px-1.5 py-0.5 text-[0.65rem] leading-none font-semibold tabular-nums"
          :class="chipClass"
        >
          {{ pctText }}
        </span>
        <span v-if="valueNow" class="text-muted-foreground text-xs tabular-nums">
          {{ valueNow }}
        </span>
      </p>
    </div>
  </a>
</template>
