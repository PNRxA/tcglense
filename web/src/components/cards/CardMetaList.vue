<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'
import { ChevronDown, ChevronUp, Coins, Sparkles } from '@lucide/vue'
import type { CardDetailOrTile } from '@/lib/api'
import { colorLettersToText } from '@/lib/mana'
import { finishLabel, frameEffectLabel, promoTypeLabel } from '@/lib/printDetails'
import { setArtist } from '@/lib/searchBuilder'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'

// The card page's "Details" list. The card is a CardDetailOrTile: the print details
// (artist, finishes, frame, Reserved List, ranks — issue #673) ride the single-card route
// only, so while the query is still seeded from a grid tile every one of them is
// `undefined` and its row simply doesn't render. Each row is therefore gated on its own
// datum, never on "the card loaded".
const props = defineProps<{ game: string; card: CardDetailOrTile }>()

// Sentence-case the group noun for the row heading ("drop" → "Drop"); a card the API predates
// the field on still reads "Drop".
const dropHeading = computed(() => {
  const noun = props.card.drop_noun ?? 'drop'
  return noun.charAt(0).toUpperCase() + noun.slice(1)
})

// Power/toughness + loyalty belong to a single-faced card as a whole; a multi-faced
// card shows them per face elsewhere, so they're suppressed in this summary.
const isMultiFace = computed(() => props.card.faces.length >= 2)

// Colour identity is a list of colour letters (["W","U"]); render it as pips.
const colorIdentityText = computed(() => colorLettersToText(props.card.color_identity))

// The mana this card can produce — the same colour-letter list, so the same pips.
const producedManaText = computed(() => colorLettersToText(props.card.produced_mana ?? []))

/** The card-browse search for this printing's illustrator — the same `a:` token the
 * advanced-search panel writes, built through the shared query builder so the quoting of
 * a name with spaces (`a:"Rebecca Guay"`) stays in one place. */
const artistSearch = computed(() => ({
  path: `/cards/${props.game}/cards`,
  query: { q: setArtist('', props.card.artist ?? '') },
}))

/** Shared chip shape; the tint per chip comes from the maps below. */
const CHIP = 'inline-flex items-center rounded-md px-1.5 py-0.5 text-xs font-medium'
const MUTED_CHIP = 'bg-muted text-foreground/80'

// A foil finish is tinted with the design system's `foil` token (the gold shimmer that
// marks a foil printing); a regular finish is a plain muted chip.
const FINISH_CHIP_CLASSES: Record<string, string> = {
  nonfoil: MUTED_CHIP,
  foil: 'bg-foil/15 text-foil',
  etched: 'bg-foil/15 text-foil',
}
const finishChipClass = (finish: string) => FINISH_CHIP_CLASSES[finish] ?? MUTED_CHIP

// "2015 · Showcase, Extended art" — the frame layout plus whatever treatments sit on it.
// Either half can be absent (a frame with no effects, or effects on a card whose frame the
// catalog doesn't carry), so the row shows whichever it has.
const frameText = computed(() => {
  const parts: string[] = []
  if (props.card.frame) parts.push(props.card.frame)
  const effects = props.card.frame_effects ?? []
  if (effects.length) parts.push(effects.map(frameEffectLabel).join(', '))
  return parts.join(' · ')
})

const promoTypes = computed(() => (props.card.promo_types ?? []).map(promoTypeLabel))

// The printing's boolean facts, as chips — only the true ones, and only when there's at
// least one. The content warning is a caution, not a neutral fact, so it takes the
// `warning` token.
const printingFlags = computed(() => {
  const card = props.card
  const flags: { label: string; class: string }[] = []
  if (card.full_art) flags.push({ label: 'Full art', class: MUTED_CHIP })
  if (card.textless) flags.push({ label: 'Textless', class: MUTED_CHIP })
  if (card.promo) flags.push({ label: 'Promo', class: MUTED_CHIP })
  if (card.variation) flags.push({ label: 'Variation', class: MUTED_CHIP })
  if (card.story_spotlight) flags.push({ label: 'Story Spotlight', class: MUTED_CHIP })
  if (card.content_warning)
    flags.push({ label: 'Content warning', class: 'bg-warning/15 text-warning' })
  return flags
})

/** Capitalise a stored one-word value for display ("borderless" → "Borderless"). */
const capitalize = (value: string) => value.charAt(0).toUpperCase() + value.slice(1)

// The list opens on the rows a player or collector reaches for first — where the printing
// is from, its cost and stats, the finishes it comes in, whether it's on the Reserved List —
// and folds the print-shop facts (frame, border, stamp, watermark, promo types, printing
// flags, produced mana, popularity ranks) behind a "Show all details" toggle, the same
// expansion CardLegalities uses. The toggle resets on a card change so an expansion can't
// leak from one card to the next inside the long-lived detail modal.
const showAll = ref(false)
watch(
  () => props.card.id,
  () => {
    showAll.value = false
  },
)

// How many secondary rows the collapsed view is hiding, for the toggle's label; a card with
// none of them shows no toggle at all.
const hiddenCount = computed(() => {
  const card = props.card
  return [
    card.produced_mana?.length,
    frameText.value,
    card.border_color,
    card.security_stamp,
    card.watermark,
    promoTypes.value.length,
    printingFlags.value.length,
    card.edhrec_rank != null,
    card.penny_rank != null,
  ].filter(Boolean).length
})
</script>

<template>
  <!-- Two label/value pairs per row once the panel is wide enough (a container query, not a
       viewport one: the same list sits in the page's main column and in the browse modal),
       one pair per row below that. -->
  <div class="@container">
    <dl
      class="grid grid-cols-[7rem_minmax(0,1fr)] gap-x-4 gap-y-2.5 text-sm @xl:grid-cols-[7rem_minmax(0,1fr)_7rem_minmax(0,1fr)]"
    >
      <dt class="text-muted-foreground">Set</dt>
      <dd>
        <RouterLink :to="`/cards/${game}/sets/${card.set_code}`" class="hover:underline">
          {{ card.set_name }} ({{ card.set_code.toUpperCase() }})
        </RouterLink>
      </dd>

      <template v-if="card.drop_name">
        <!-- The row is headed by what the group *is* — "Drop" for a Secret Lair drop,
             "Treatment" for one of The Zeta Set's print-treatment sections — off the card's
             `drop_noun`, so a treatment is never presented as a drop. -->
        <dt class="text-muted-foreground">{{ dropHeading }}</dt>
        <dd class="flex flex-wrap items-center gap-x-2 gap-y-1">
          <!-- Link the card to its Secret Lair drop (the set's by-drop view, filtered to
               this drop). Falls back to plain text on the rare drop with no slug. -->
          <RouterLink
            v-if="card.drop_slug"
            :to="{ path: `/cards/${game}/sets/${card.set_code}`, query: { drop: card.drop_name } }"
            class="hover:underline"
          >
            {{ card.drop_name }}
          </RouterLink>
          <span v-else>{{ card.drop_name }}</span>
          <!-- Chase / bonus card: the optional card given with a qualifying drop purchase,
               with no sealed product of its own (issue #295). A spend reward is the more
               specific label, so it's suppressed here and shown in its own row below. -->
          <span
            v-if="card.secret_lair_bonus && !card.secret_lair_spend_incentive"
            class="bg-warning/15 text-warning inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-xs font-semibold"
            title="Chase card — the optional bonus card given with a qualifying Secret Lair purchase."
          >
            <Sparkles class="size-3" />
            Chase card
          </span>
        </dd>
      </template>

      <!-- Spend reward: a promo handed out for reaching a cart spend threshold during the
           superdrop, not tied to a single drop (issue #331). Its own row so it shows even for
           a promo the drop snapshot doesn't group (e.g. one added after the snapshot). -->
      <template v-if="card.secret_lair_spend_incentive">
        <dt class="text-muted-foreground">Promo</dt>
        <dd>
          <span
            class="bg-success/15 text-success inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-xs font-semibold"
            title="Spend reward — a promo card given for reaching a spend threshold during the Secret Lair superdrop, not included with a single drop."
          >
            <Coins class="size-3" />
            Spend reward
          </span>
        </dd>
      </template>

      <dt class="text-muted-foreground">Number</dt>
      <dd>#{{ card.collector_number }}</dd>

      <template v-if="card.rarity">
        <dt class="text-muted-foreground">Rarity</dt>
        <dd class="capitalize">{{ card.rarity }}</dd>
      </template>

      <template v-if="card.artist">
        <dt class="text-muted-foreground">Artist</dt>
        <dd>
          <!-- Every printing this illustrator painted, one click away. A multi-artist card
               credits all of them in the one string, as printed, so the link searches that
               exact credit. -->
          <RouterLink :to="artistSearch" class="hover:underline">{{ card.artist }}</RouterLink>
        </dd>
      </template>

      <template v-if="card.mana_cost">
        <dt class="text-muted-foreground">Mana cost</dt>
        <dd><ManaSymbols :text="card.mana_cost" /></dd>
      </template>

      <template v-if="card.color_identity.length">
        <dt class="text-muted-foreground">Color identity</dt>
        <dd><ManaSymbols :text="colorIdentityText" /></dd>
      </template>

      <template v-if="!isMultiFace && card.power && card.toughness">
        <dt class="text-muted-foreground">Power / Toughness</dt>
        <dd class="tabular-nums">{{ card.power }} / {{ card.toughness }}</dd>
      </template>

      <template v-if="!isMultiFace && card.loyalty">
        <dt class="text-muted-foreground">Loyalty</dt>
        <dd class="tabular-nums">{{ card.loyalty }}</dd>
      </template>

      <!-- Defense is NOT gated on `!isMultiFace` like the two rows above: a Battle is a
           two-faced card (a transform back face), and the wire carries the defense box only
           at the top level — the per-face shape has none — so gating it the same way would
           hide the box every Battle actually prints. Rendered verbatim, like P/T. -->
      <template v-if="card.defense">
        <dt class="text-muted-foreground">Defense</dt>
        <dd class="tabular-nums">{{ card.defense }}</dd>
      </template>

      <template v-if="card.finishes?.length">
        <dt class="text-muted-foreground">Finishes</dt>
        <dd class="flex flex-wrap gap-1">
          <span
            v-for="finish in card.finishes"
            :key="finish"
            :class="[CHIP, finishChipClass(finish)]"
          >
            {{ finishLabel(finish) }}
          </span>
        </dd>
      </template>

      <!-- The Reserved List gets its own row rather than a "Printing" chip: it's a promise
           about the card's future, not a fact about this printing's ink — and a collector's
           first question, so it stays in the collapsed view. Neutral-but-notable, so the
           `info` token. -->
      <template v-if="card.reserved">
        <dt class="text-muted-foreground">Reserved List</dt>
        <dd>
          <span :class="[CHIP, 'bg-info/15 text-info']">Never to be reprinted</span>
        </dd>
      </template>

      <template v-if="card.released_at">
        <dt class="text-muted-foreground">Released</dt>
        <dd>{{ card.released_at }}</dd>
      </template>

      <!-- The print-shop facts, folded away until asked for. -->
      <template v-if="showAll">
        <template v-if="card.produced_mana?.length">
          <dt class="text-muted-foreground">Produces</dt>
          <dd><ManaSymbols :text="producedManaText" /></dd>
        </template>

        <template v-if="frameText">
          <dt class="text-muted-foreground">Frame</dt>
          <dd>{{ frameText }}</dd>
        </template>

        <template v-if="card.border_color">
          <dt class="text-muted-foreground">Border</dt>
          <dd>{{ capitalize(card.border_color) }}</dd>
        </template>

        <template v-if="card.security_stamp">
          <dt class="text-muted-foreground">Stamp</dt>
          <dd>{{ capitalize(card.security_stamp) }}</dd>
        </template>

        <template v-if="card.watermark">
          <dt class="text-muted-foreground">Watermark</dt>
          <dd>{{ capitalize(card.watermark) }}</dd>
        </template>

        <template v-if="promoTypes.length">
          <dt class="text-muted-foreground">Promo types</dt>
          <dd class="flex flex-wrap gap-1">
            <span v-for="promo in promoTypes" :key="promo" :class="[CHIP, MUTED_CHIP]">
              {{ promo }}
            </span>
          </dd>
        </template>

        <template v-if="printingFlags.length">
          <dt class="text-muted-foreground">Printing</dt>
          <dd class="flex flex-wrap gap-1">
            <span v-for="flag in printingFlags" :key="flag.label" :class="[CHIP, flag.class]">
              {{ flag.label }}
            </span>
          </dd>
        </template>

        <template v-if="card.edhrec_rank != null">
          <dt class="text-muted-foreground">EDHREC rank</dt>
          <dd class="tabular-nums">#{{ card.edhrec_rank.toLocaleString() }}</dd>
        </template>

        <template v-if="card.penny_rank != null">
          <dt class="text-muted-foreground">Penny rank</dt>
          <dd class="tabular-nums">#{{ card.penny_rank.toLocaleString() }}</dd>
        </template>
      </template>
    </dl>

    <button
      v-if="hiddenCount"
      type="button"
      class="text-muted-foreground hover:text-foreground mt-3 inline-flex items-center gap-1 text-xs font-medium"
      :aria-expanded="showAll"
      @click="showAll = !showAll"
    >
      <component :is="showAll ? ChevronUp : ChevronDown" class="size-3.5" aria-hidden="true" />
      {{ showAll ? 'Show fewer details' : `Show all details (${hiddenCount} more)` }}
    </button>
  </div>
</template>
