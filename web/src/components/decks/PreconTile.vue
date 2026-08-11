<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import { Layers } from '@lucide/vue'
import CardImage from '@/components/cards/CardImage.vue'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'
import { colorLettersToText } from '@/lib/mana'
import type { PreconDeck } from '@/lib/api'

// One preconstructed deck in the browse grid: the card that fronts it, its name, and the
// line that says what it is (type · set · size).
//
// The face card is the deck's commander when it has one, else the first card the publisher
// lists — which for a Secret Lair drop is the drop's own leading card, so the tile looks
// like the product. It rides the list response, so drawing it costs no extra request.
//
// Colours follow `DeckIdentity`'s three-way convention exactly, because the server folds
// them by the same rule: `null` = nothing to read a colour off (don't claim anything), `[]`
// = genuinely colourless (`{C}`), letters = those pips.
const props = defineProps<{ precon: PreconDeck; game: string }>()

const identityText = computed(() => {
  const letters = props.precon.color_identity
  if (letters === null) return ''
  return letters.length ? colorLettersToText(letters) : '{C}'
})

const COLOUR_NAMES: Readonly<Record<string, string>> = {
  W: 'white',
  U: 'blue',
  B: 'black',
  R: 'red',
  G: 'green',
}

const identityLabel = computed(() => {
  const letters = props.precon.color_identity
  if (letters === null) return ''
  const named = letters.map((letter) => COLOUR_NAMES[letter] ?? letter)
  return `Colour identity: ${named.length ? named.join(', ') : 'colourless'}`
})

/** "2026" from an ISO date — the year is what dates a precon on a tile. */
const year = computed(() => props.precon.released_at?.slice(0, 4) ?? '')
</script>

<template>
  <RouterLink
    :to="`/decks/${game}/precons/${precon.slug}`"
    class="bg-card hover:border-primary/50 group flex gap-3 rounded-lg border p-3 transition"
  >
    <div class="w-20 shrink-0 sm:w-24">
      <CardImage
        v-if="precon.face_card"
        :game="game"
        :id="precon.face_card.card_id"
        :name="precon.face_card.name"
        :has-image="precon.face_card.has_image"
        size="small"
      />
      <!-- No face card (its printing left the catalog): keep the tile's shape rather than
        collapsing the row. -->
      <div
        v-else
        class="bg-muted text-muted-foreground flex aspect-[61/85] items-center justify-center rounded-md"
      >
        <Layers class="size-5" aria-hidden="true" />
      </div>
    </div>

    <div class="flex min-w-0 flex-col justify-center">
      <p class="line-clamp-2 font-medium" :title="precon.name">{{ precon.name }}</p>
      <p v-if="identityText" class="mt-1">
        <ManaSymbols
          :text="identityText"
          class="leading-none"
          role="img"
          :aria-label="identityLabel"
        />
      </p>
      <p class="text-muted-foreground mt-1 truncate text-sm" :title="precon.set_name ?? undefined">
        {{ precon.deck_type }}
        <span v-if="precon.set_name"> · {{ precon.set_name }}</span>
        <span v-else-if="precon.set_code"> · {{ precon.set_code.toUpperCase() }}</span>
      </p>
      <p class="text-muted-foreground text-sm">
        {{ precon.card_count }} card{{ precon.card_count === 1 ? '' : 's' }}
        <span v-if="precon.sideboard_count"> · +{{ precon.sideboard_count }} sideboard</span>
        <span v-if="year"> · {{ year }}</span>
      </p>
    </div>
  </RouterLink>
</template>
