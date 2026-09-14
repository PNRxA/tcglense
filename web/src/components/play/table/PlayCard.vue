<script setup lang="ts">
import { computed } from 'vue'
import { cardImageUrl, type ImageSize } from '@/lib/api'
import { SITE_NAME } from '@/lib/seo'
import { cardAriaLabel, cardName, counterChips } from '@/lib/playTable'
import type { PlayCardView } from '@/lib/api/play'

// One card, anywhere on the table.
//
// **Purely presentational on purpose.** Every gesture the table understands (press, drag,
// double-click, hover preview, the context menu) is wired by whichever surface is rendering
// the card — the battlefield, the hand, a zone viewer, an opponent's board — and lands here
// as a plain fallthrough listener on the single root `<button>`. That keeps the interaction
// model in one place (`usePlayTable`) instead of smeared across a component that is mounted
// a hundred times.
//
// Three renderings, decided in this order:
//
// 1. **A back** — either because the card is face down, or because the viewer simply isn't
//    allowed to know what it is (`def: null`, which is what the server sends for someone
//    else's face-down permanent). It is drawn, never fetched: there is no "card back" image
//    in the catalog, and requesting one per card would be a hundred pointless requests.
// 2. **A text card** — an ad-hoc token typed in at the table has no printing to show, so it
//    renders as what it is: a name, a type line and a P/T inside a dashed border, which also
//    reads as "this is a token" at a glance.
// 3. **The printing** — through the catalog image proxy, asking for face 1 only when the
//    card actually has a distinct back image (a transforming card); an MDFC whose second face
//    shares the front's art would 404 a `?face=1`.
const props = withDefaults(
  defineProps<{
    card: PlayCardView
    /** Catalog game slug for the image URL (the room's, unless the def carries its own). */
    game: string
    size?: ImageSize
    /** False on an opponent's board: the card is shown, not operated. */
    interactive?: boolean
    selected?: boolean
    /** Dims the card while it is being dragged away from here. */
    dragging?: boolean
  }>(),
  { size: 'normal', interactive: true, selected: false, dragging: false },
)

const def = computed(() => props.card.def)
/** A back: face down, or an identity this viewer was never given. */
const back = computed(() => props.card.face_down || !def.value)
/** Rendered as text: an ad-hoc token, or a printing the catalog has no image for. */
const textCard = computed(
  () => !back.value && def.value !== null && (def.value.card_id === null || !def.value.has_image),
)

const face = computed(() => {
  const faces = def.value?.faces ?? []
  return faces[props.card.face_index] ?? faces[0] ?? null
})

const imageUrl = computed(() => {
  const d = def.value
  if (!d?.card_id || !d.has_image) return null
  // `back_image` is the wire's answer to "does face 1 have art of its own"; without it the
  // front image is the right picture for both faces.
  const faceParam = props.card.face_index === 1 && d.back_image ? 1 : undefined
  return cardImageUrl(d.game || props.game, d.card_id, props.size, faceParam)
})

const label = computed(() => cardAriaLabel(props.card))
const chips = computed(() => counterChips(props.card))
const powerToughness = computed(() => {
  if (props.card.power_toughness) return props.card.power_toughness
  const f = face.value
  return f?.power !== null && f?.power !== undefined && f.toughness
    ? `${f.power}/${f.toughness}`
    : null
})
</script>

<template>
  <!-- A button when it can be operated, a labelled image when it cannot. Not always a button:
    a pile (`PlayZonePile`) is itself a button showing its top card, and a button inside a
    button is invalid markup that browsers resolve by dropping one of them. -->
  <component
    :is="interactive ? 'button' : 'div'"
    :type="interactive ? 'button' : undefined"
    :role="interactive ? undefined : 'img'"
    class="group relative block w-full touch-none select-none focus-visible:outline-none"
    :class="dragging ? 'opacity-40' : ''"
    :data-play-card="card.id"
    :aria-label="label"
  >
    <!-- The whole card turns, not just its art: a tapped permanent reads as tapped from
      across the table, which is the only reason anyone taps anything. -->
    <div
      class="transition-transform duration-150 motion-reduce:transition-none"
      :class="card.tapped ? 'rotate-90' : ''"
    >
      <div
        class="shadow-card ring-offset-background relative aspect-[61/85] overflow-hidden rounded-[4.76%_/_3.42%]"
        :class="[
          selected ? 'ring-ring ring-2 ring-offset-1' : '',
          interactive ? 'group-focus-visible:ring-ring group-focus-visible:ring-2' : '',
        ]"
      >
        <!-- 1. A back. -->
        <div
          v-if="back"
          class="bg-muted text-muted-foreground/70 border-border/60 absolute inset-0 flex items-center justify-center border"
        >
          <span class="rotate-[-20deg] text-[0.5rem] font-semibold tracking-[0.2em] uppercase">
            {{ SITE_NAME }}
          </span>
        </div>
        <!-- 2. A typed-in token: no printing exists, so the text is the card. -->
        <div
          v-else-if="textCard"
          class="bg-card border-muted-foreground/50 absolute inset-0 flex flex-col justify-between rounded-[4.76%_/_3.42%] border border-dashed p-1.5 text-center"
        >
          <span class="text-[0.6rem] leading-tight font-semibold">{{ cardName(card) }}</span>
          <span class="text-muted-foreground text-[0.5rem] leading-tight">
            {{ face?.type_line ?? 'Token' }}
          </span>
          <span class="text-[0.6rem] leading-none font-semibold tabular-nums">
            {{ powerToughness ?? '' }}
          </span>
        </div>
        <!-- 3. The printing. -->
        <img
          v-else-if="imageUrl"
          :src="imageUrl"
          :alt="cardName(card)"
          loading="lazy"
          draggable="false"
          class="h-full w-full object-contain"
        />
        <div
          v-else
          class="bg-muted text-muted-foreground absolute inset-0 grid place-items-center p-1 text-center text-[0.6rem] leading-tight"
        >
          {{ cardName(card) }}
        </div>
      </div>
    </div>

    <!-- Counters ride the card itself rather than a tooltip: a 4/4 with three +1/+1 counters
      has to be readable while you're looking at the board, not after a hover. -->
    <div
      v-if="chips.length > 0"
      class="pointer-events-none absolute inset-x-0 bottom-0 flex flex-wrap justify-center gap-0.5 p-0.5"
      aria-hidden="true"
    >
      <span
        v-for="chip in chips"
        :key="chip.name"
        class="bg-foreground text-background rounded px-1 text-[0.55rem] leading-4 font-semibold tabular-nums"
      >
        {{ chip.name }} {{ chip.value > 0 ? `+${chip.value}` : chip.value }}
      </span>
    </div>
    <span
      v-if="card.revealed"
      class="bg-info/15 text-info pointer-events-none absolute top-0.5 left-0.5 rounded px-1 text-[0.5rem] font-semibold uppercase"
      aria-hidden="true"
      >Revealed</span
    >
  </component>
</template>
