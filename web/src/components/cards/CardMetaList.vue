<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import type { Card } from '@/lib/api'
import { colorLettersToText } from '@/lib/mana'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'

const props = defineProps<{ game: string; card: Card }>()

// Power/toughness + loyalty belong to a single-faced card as a whole; a multi-faced
// card shows them per face elsewhere, so they're suppressed in this summary.
const isMultiFace = computed(() => props.card.faces.length >= 2)

// Colour identity is a list of colour letters (["W","U"]); render it as pips.
const colorIdentityText = computed(() => colorLettersToText(props.card.color_identity))
</script>

<template>
  <dl class="mt-6 grid grid-cols-[8rem_1fr] gap-x-4 gap-y-2 text-sm">
    <dt class="text-muted-foreground">Set</dt>
    <dd>
      <RouterLink :to="`/cards/${game}/sets/${card.set_code}`" class="hover:underline">
        {{ card.set_name }} ({{ card.set_code.toUpperCase() }})
      </RouterLink>
    </dd>

    <template v-if="card.drop_name">
      <dt class="text-muted-foreground">Drop</dt>
      <dd>{{ card.drop_name }}</dd>
    </template>

    <dt class="text-muted-foreground">Number</dt>
    <dd>#{{ card.collector_number }}</dd>

    <template v-if="card.rarity">
      <dt class="text-muted-foreground">Rarity</dt>
      <dd class="capitalize">{{ card.rarity }}</dd>
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

    <template v-if="card.released_at">
      <dt class="text-muted-foreground">Released</dt>
      <dd>{{ card.released_at }}</dd>
    </template>
  </dl>
</template>
