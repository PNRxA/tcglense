<script setup lang="ts">
import { computed } from 'vue'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'
import { colorLettersToText } from '@/lib/mana'
import type { Deck } from '@/lib/api'

// The "what is this deck" line under a deck's name in a deck list: its colour identity as
// mana pips, and the card leading it. Both ride the `Deck` header the list already fetched,
// so this adds no request.
//
// Shared by the private list's `DeckTile` and the public profile's deck cards — the same
// deck must read the same way whether you own it or found it.
//
// The blank/colourless split is the server's, never inferred from `card_count`: that counts
// the sideboard, which deliberately doesn't colour a deck, so a sideboard-only deck has cards
// AND nothing to say about colour. `color_identity: null` is exactly that case; `[]` is a
// deck that genuinely plays no colour.
const props = defineProps<{ deck: Deck }>()

const commanders = computed(() => props.deck.commanders)

/** Nothing to show: no colour to read, and no card leading the deck. */
const isBlank = computed(() => props.deck.color_identity === null && commanders.value.length === 0)

/** `{C}` for a deck that plays no colour; nothing at all when there was nothing to judge. */
const identityText = computed(() => {
  const letters = props.deck.color_identity
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

/** "Colour identity: white, blue". The pips carry per-symbol labels of their own, but with
 *  no framing a screen reader reads a deck tile as a bare list of "… mana" fragments. */
const identityLabel = computed(() => {
  const letters = props.deck.color_identity
  if (letters === null) return ''
  const named = letters.map((letter) => COLOUR_NAMES[letter] ?? letter)
  return `Colour identity: ${named.length ? named.join(', ') : 'colourless'}`
})

/** "Tana & Tymna" for a partner pair. Every name the header carries is joined — the API
 *  already caps how many it sends, and the row truncates with the full text on `title`, so
 *  neither layer has to print a count that the other one might make wrong. */
const commanderLabel = computed(() => commanders.value.map((c) => c.name).join(' & '))
</script>

<template>
  <p v-if="!isBlank" class="mt-1 flex min-w-0 items-center gap-1.5 text-sm">
    <ManaSymbols
      v-if="identityText"
      :text="identityText"
      class="shrink-0 leading-none"
      role="img"
      :aria-label="identityLabel"
    />
    <span v-if="commanderLabel" class="text-muted-foreground truncate" :title="commanderLabel">
      <span class="sr-only">Commander: </span>{{ commanderLabel }}
    </span>
  </p>
</template>
