<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import CommanderPickerField from '@/components/life/CommanderPickerField.vue'
import DeckPickerField from '@/components/life/DeckPickerField.vue'

// What a seat was playing — one of your decks, or just a commander.
//
// The two are alternatives (the server refuses both), so this is one control with a mode switch
// rather than two fields that can disagree. The split matters at a real table: a deck link is for
// *you*, and builds that deck's win record; a commander is for everyone else, whose decks you'll
// never have but whose commander you always know.
const props = defineProps<{
  game: string
  deckId: number | null
  commanderCardId: string | null
  commanderName?: string | null
  /** Distinguishes this seat's controls for a screen reader. */
  seatLabel?: string
}>()

const emit = defineEmits<{
  'update:deckId': [value: number | null]
  'update:commanderCardId': [value: string | null]
}>()

type Mode = 'none' | 'deck' | 'commander'

/** Open on whichever link the seat already has, else on "no link". */
function modeOf(): Mode {
  if (props.deckId !== null) return 'deck'
  if (props.commanderCardId !== null) return 'commander'
  return 'none'
}

const mode = ref<Mode>(modeOf())
watch(
  () => [props.deckId, props.commanderCardId],
  () => {
    const derived = modeOf()
    // Only follow the props when they still name a link. A link going null is almost always this
    // control clearing the *other* one as the user switches mode: following that back would
    // re-derive "none" and snap the toggle to "Neither", discarding the choice just made.
    if (derived !== 'none') mode.value = derived
  },
)
// A reused instance (a seat removed from a v-for above this one) is a different seat, so its
// mode is re-seeded from that seat's links rather than inherited.
watch(
  () => props.seatLabel,
  () => (mode.value = modeOf()),
)

const modeModel = computed({
  get: () => mode.value,
  set: (value: string) => {
    if (!value) return
    mode.value = value as Mode
    // Switching mode clears the other link, so the pair can never both be set — the same rule
    // the server enforces, applied before the request rather than after the 422.
    if (value !== 'deck') emit('update:deckId', null)
    if (value !== 'commander') emit('update:commanderCardId', null)
  },
})
</script>

<template>
  <div class="space-y-2">
    <ToggleGroup v-model="modeModel" type="single" variant="outline" size="sm">
      <ToggleGroupItem
        value="none"
        :aria-label="`No deck or commander${seatLabel ? ` for ${seatLabel}` : ''}`"
      >
        Neither
      </ToggleGroupItem>
      <ToggleGroupItem
        value="deck"
        :aria-label="`Link a deck${seatLabel ? ` for ${seatLabel}` : ''}`"
      >
        My deck
      </ToggleGroupItem>
      <ToggleGroupItem
        value="commander"
        :aria-label="`Name a commander${seatLabel ? ` for ${seatLabel}` : ''}`"
      >
        Commander
      </ToggleGroupItem>
    </ToggleGroup>

    <DeckPickerField
      v-if="mode === 'deck'"
      :model-value="deckId"
      :game="game"
      :label="seatLabel ? `Deck for ${seatLabel}` : 'Deck'"
      @update:model-value="(value) => emit('update:deckId', value)"
    />
    <CommanderPickerField
      v-else-if="mode === 'commander'"
      :model-value="commanderCardId"
      :name="commanderName"
      :game="game"
      :label="seatLabel ? `Commander for ${seatLabel}` : 'Commander'"
      @update:model-value="(value) => emit('update:commanderCardId', value)"
    />
    <p v-else class="text-muted-foreground text-xs">
      Counts life only — nothing is added to any deck's record.
    </p>
  </div>
</template>
