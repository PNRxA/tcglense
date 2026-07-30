<script setup lang="ts">
import { computed } from 'vue'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useDecksQuery } from '@/composables/useDecks'
import { toRef } from 'vue'

// Pick which of your decks a seat played — the link that turns a life counter into a win record.
//
// "No deck" is first-class and the default: most games at a kitchen table aren't played with a
// deck you've built in TCGLense, and a tool that nags for one would be worse than one that
// doesn't ask. A seat with no deck still counts life and still appears in the history; it just
// contributes to no deck's record.
const props = defineProps<{ game: string; modelValue: number | null; label?: string }>()
const emit = defineEmits<{ 'update:modelValue': [value: number | null] }>()

const NO_DECK = 'none'

const { data, isPending } = useDecksQuery(toRef(props, 'game'))
const decks = computed(() => data.value?.data ?? [])

const selected = computed({
  get: () => (props.modelValue === null ? NO_DECK : String(props.modelValue)),
  set: (value: string) => emit('update:modelValue', value === NO_DECK ? null : Number(value)),
})
</script>

<template>
  <Select v-model="selected">
    <SelectTrigger class="w-full" :aria-label="label ?? 'Deck'">
      <SelectValue placeholder="No deck" />
    </SelectTrigger>
    <SelectContent>
      <SelectItem :value="NO_DECK">No deck</SelectItem>
      <SelectItem v-for="deck in decks" :key="deck.id" :value="String(deck.id)">
        {{ deck.name }}
      </SelectItem>
      <!-- Say why the list is empty rather than showing a bare "No deck" and looking broken. -->
      <SelectItem v-if="!isPending && !decks.length" value="empty" disabled>
        You have no decks for this game yet
      </SelectItem>
    </SelectContent>
  </Select>
</template>
