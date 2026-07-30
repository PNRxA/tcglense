<script setup lang="ts">
import { computed, onScopeDispose, ref } from 'vue'
import { Crown, Minus, Plus, Settings2, Skull } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import LifeSparkline from '@/components/life/LifeSparkline.vue'
import { DANGER_LIFE, rotationClass } from '@/lib/lifeLayout'
import { seatColor } from '@/lib/lifeSeries'
import type { LifeSeatView } from '@/composables/useLifeSession'

// One seat on the mat: a name, a very large total, and two full-height tap zones.
//
// Three decisions worth stating:
//
// 1. **The tap zones are the tile.** Left half is −1, right half is +1, floor to ceiling — the
//    thing you do a hundred times a game needs no aiming. The controls that you touch once
//    (settings) are small and cornered so a stray thumb can't hit them.
// 2. **The content rotates, not the cell.** The tile is sized from the container-query vars the
//    grid cell sets, so a quarter-turned seat still fills its cell — and because the tap zones
//    are inside the rotated content, "up" is up from where that player is sitting.
// 3. **Holding repeats.** A 12-point hit is one press, not twelve taps. The repeat only starts
//    after a deliberate pause so a normal tap is still exactly one point.
const props = defineProps<{
  view: LifeSeatView
  /** False for a finished game: the mat becomes read-only. */
  editable: boolean
  /** Marks the winning seat once a result is recorded. */
  winner: boolean
  gameSlug: string
}>()

const emit = defineEmits<{ bump: [delta: number]; settings: [] }>()

/** How long a press waits before it starts repeating, and how fast it repeats after that. */
const HOLD_DELAY_MS = 400
const HOLD_INTERVAL_MS = 90

let holdTimer: ReturnType<typeof setTimeout> | undefined
let holdRepeat: ReturnType<typeof setInterval> | undefined
const holding = ref(0)

function stopHold() {
  if (holdTimer !== undefined) clearTimeout(holdTimer)
  if (holdRepeat !== undefined) clearInterval(holdRepeat)
  holdTimer = undefined
  holdRepeat = undefined
  holding.value = 0
}

function startHold(delta: number) {
  if (!props.editable) return
  stopHold()
  // The first point lands on press, not on release, so the tile feels immediate.
  emit('bump', delta)
  holding.value = delta
  holdTimer = setTimeout(() => {
    holdRepeat = setInterval(() => emit('bump', delta), HOLD_INTERVAL_MS)
  }, HOLD_DELAY_MS)
}

onScopeDispose(stopHold)

const dead = computed(() => props.view.life <= 0)
const danger = computed(() => !dead.value && props.view.life <= DANGER_LIFE)
const accent = computed(() => seatColor(props.view.seat.position))

const lifeClass = computed(() => {
  if (dead.value) return 'text-destructive'
  if (danger.value) return 'text-amber-600 dark:text-amber-400'
  return ''
})

const tileClass = computed(() => {
  if (props.winner) return 'border-emerald-500/50 bg-emerald-500/10'
  if (dead.value) return 'border-destructive/40 bg-destructive/5'
  if (danger.value) return 'border-amber-500/40'
  return ''
})

const pendingLabel = computed(() =>
  props.view.pending > 0 ? `+${props.view.pending}` : String(props.view.pending),
)

/** What a screen reader hears when the total changes — name and number, never colour alone. */
const announcement = computed(() => `${props.view.seat.name}: ${props.view.life} life`)
</script>

<template>
  <div
    class="bg-card absolute top-1/2 left-1/2 flex -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden rounded-xl border"
    :class="[rotationClass(view.placement.rotation), tileClass]"
    :style="{ width: 'var(--life-tile-w, 100%)', height: 'var(--life-tile-h, 100%)' }"
  >
    <!-- Seat identity. A thin coloured rule ties the tile to its line in the history chart;
         the name beside it is what actually identifies the seat. -->
    <div class="flex items-center gap-2 px-3 pt-2">
      <span
        class="size-2 shrink-0 rounded-full"
        :style="{ backgroundColor: accent }"
        aria-hidden="true"
      />
      <div class="min-w-0 flex-1">
        <p class="truncate text-sm font-medium leading-tight">{{ view.seat.name }}</p>
        <RouterLink
          v-if="view.seat.deck_id !== null && view.seat.deck_name"
          :to="`/decks/${gameSlug}/${view.seat.deck_id}`"
          class="text-muted-foreground hover:text-foreground block truncate text-xs hover:underline"
          @click.stop
        >
          {{ view.seat.deck_name }}
        </RouterLink>
        <!-- A commander instead of a deck: the same slot, linked to the card rather than a deck
             page (it's someone else's list, so there's no deck of ours to open). -->
        <RouterLink
          v-else-if="view.seat.commander_card_id && view.seat.commander_name"
          :to="{ query: { ...$route.query, card: view.seat.commander_card_id } }"
          class="text-muted-foreground hover:text-foreground block truncate text-xs hover:underline"
          @click.stop
        >
          {{ view.seat.commander_name }}
        </RouterLink>
      </div>
      <Crown
        v-if="winner"
        class="size-4 shrink-0 text-emerald-600 dark:text-emerald-400"
        aria-label="Winner"
      />
      <Skull v-else-if="dead" class="text-destructive size-4 shrink-0" aria-label="Out of life" />
      <button
        type="button"
        class="text-muted-foreground hover:text-foreground hover:bg-accent -mr-1 grid size-7 shrink-0 place-items-center rounded-md"
        :aria-label="`Seat settings for ${view.seat.name}`"
        @click="emit('settings')"
      >
        <Settings2 class="size-4" aria-hidden="true" />
      </button>
    </div>

    <!-- The total, with the two tap zones behind it. -->
    <div class="relative min-h-0 flex-1">
      <template v-if="editable">
        <button
          type="button"
          class="group absolute inset-y-0 left-0 w-1/2 cursor-pointer select-none"
          :aria-label="`Lose a life for ${view.seat.name}`"
          @pointerdown.prevent="startHold(-1)"
          @pointerup="stopHold"
          @pointercancel="stopHold"
          @pointerleave="stopHold"
          @keydown.enter.prevent="emit('bump', -1)"
          @keydown.space.prevent="emit('bump', -1)"
        >
          <Minus
            class="text-muted-foreground/50 group-hover:text-foreground/70 absolute top-1/2 left-3 size-5 -translate-y-1/2 transition-colors"
            aria-hidden="true"
          />
        </button>
        <button
          type="button"
          class="group absolute inset-y-0 right-0 w-1/2 cursor-pointer select-none"
          :aria-label="`Gain a life for ${view.seat.name}`"
          @pointerdown.prevent="startHold(1)"
          @pointerup="stopHold"
          @pointercancel="stopHold"
          @pointerleave="stopHold"
          @keydown.enter.prevent="emit('bump', 1)"
          @keydown.space.prevent="emit('bump', 1)"
        >
          <Plus
            class="text-muted-foreground/50 group-hover:text-foreground/70 absolute top-1/2 right-3 size-5 -translate-y-1/2 transition-colors"
            aria-hidden="true"
          />
        </button>
      </template>

      <div class="pointer-events-none absolute inset-0 flex flex-col items-center justify-center">
        <span
          class="text-[clamp(2.5rem,28cqmin,7rem)] font-semibold tabular-nums leading-none tracking-tight"
          :class="lifeClass"
          >{{ view.life }}</span
        >
        <!-- The uncommitted run of taps, shown until it commits — so a fast −5 reads as one
             change in progress rather than a total that jumped for no visible reason. -->
        <span
          v-if="view.pending !== 0"
          class="bg-foreground/10 mt-1.5 rounded-full px-2 py-0.5 text-xs font-medium tabular-nums"
          >{{ pendingLabel }}</span
        >
      </div>
      <!-- One polite live region per seat: the committed total, announced without stealing focus. -->
      <span class="sr-only" aria-live="polite">{{ announcement }}</span>
    </div>

    <!-- The seat's own history, at a glance. -->
    <div class="h-6 px-2 pb-1.5 opacity-70">
      <LifeSparkline :line="view.line" :position="view.seat.position" />
    </div>
  </div>
</template>
