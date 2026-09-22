<script setup lang="ts">
import { computed, ref } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cardImageUrl, listCards } from '@/lib/api'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { PLAY_TOKEN_COUNT_MAX } from '@/lib/playTable'
import type { Card } from '@/lib/api'

// Making a token — the one thing at a manual table that has no card to move.
//
// Two ways in, because tokens come in two kinds. Most have a **printing**: a 1/1 white Soldier
// exists in the catalog with art, and finding it is a normal card search narrowed to
// `t:token`. Some don't — the ones a card invents on the spot, the ones nobody has the token
// for — so the second tab builds a text card from four fields. Both end in the same
// `create_token`; the difference is whether a `card_id` rides along, and therefore whether the
// table draws art or draws text.
//
// Count is capped at the engine's own limit rather than left open: twenty is already a
// Sliver deck's worst turn, and an uncapped number field on a shared table is an accident
// waiting to happen.
const table = usePlayTableContext()

const mode = ref<'search' | 'text'>('search')
const term = ref('')
const count = ref(1)

const name = ref('')
const typeLine = ref('Token Creature')
const powerToughness = ref('1/1')
const colors = ref('')

const trimmed = computed(() => term.value.trim())
const search = useQuery({
  // The term rides *inside* the key as a computed, so the query re-runs as it is typed.
  queryKey: ['play', 'tokens', computed(() => table.store.game), trimmed],
  queryFn: ({ signal }) =>
    listCards(table.store.game, { q: `t:token ${trimmed.value}`, pageSize: 12 }, signal),
  enabled: computed(() => trimmed.value.length >= 2),
  staleTime: 5 * 60_000,
})

const results = computed<Card[]>(() => search.data.value?.data ?? [])

function clampedCount(): number {
  const value = Number(count.value)
  if (!Number.isFinite(value)) return 1
  return Math.min(PLAY_TOKEN_COUNT_MAX, Math.max(1, Math.round(value)))
}

function createFromCard(card: Card) {
  table.send({
    type: 'create_token',
    name: card.name,
    card_id: card.id,
    type_line: card.type_line ?? null,
    power_toughness: card.power && card.toughness ? `${card.power}/${card.toughness}` : null,
    colors: (card.colors ?? []).slice(),
    x: 0.5,
    y: 0.65,
    count: clampedCount(),
  })
  close()
}

function createFromText() {
  const label = name.value.trim()
  if (!label) return
  table.send({
    type: 'create_token',
    name: label.slice(0, 64),
    card_id: null,
    type_line: typeLine.value.trim() || null,
    power_toughness: powerToughness.value.trim() || null,
    colors: colors.value
      .toUpperCase()
      .split('')
      .filter((letter) => 'WUBRG'.includes(letter)),
    x: 0.5,
    y: 0.65,
    count: clampedCount(),
  })
  close()
}

function close() {
  table.tokenOpen.value = false
}
</script>

<template>
  <Dialog :open="table.tokenOpen.value" @update:open="(value: boolean) => !value && close()">
    <DialogContent
      anchor="top"
      class="bg-background flex max-h-[85dvh] w-[min(94vw,44rem)] flex-col rounded-xl border p-5 shadow-xl"
    >
      <DialogTitle>Make a token</DialogTitle>
      <DialogDescription>
        Find the printing, or write one out if the catalog doesn't have it.
      </DialogDescription>

      <div class="mt-3 flex gap-1" role="group" aria-label="How to make the token">
        <Button
          :variant="mode === 'search' ? 'default' : 'outline'"
          size="sm"
          @click="mode = 'search'"
          >Find a printing</Button
        >
        <Button :variant="mode === 'text' ? 'default' : 'outline'" size="sm" @click="mode = 'text'"
          >Write one out</Button
        >
        <label class="ml-auto flex items-center gap-1 text-sm">
          <span class="text-muted-foreground">How many</span>
          <Input
            v-model="count"
            type="number"
            min="1"
            :max="PLAY_TOKEN_COUNT_MAX"
            class="h-8 w-16"
            aria-label="How many tokens"
          />
        </label>
      </div>

      <template v-if="mode === 'search'">
        <Input
          v-model="term"
          class="mt-3"
          placeholder="Soldier, Treasure, Beast…"
          aria-label="Token name"
        />
        <div class="mt-3 min-h-0 flex-1 overflow-y-auto">
          <p v-if="trimmed.length < 2" class="text-muted-foreground text-sm">
            Type at least two letters.
          </p>
          <p v-else-if="search.isPending.value" class="text-muted-foreground text-sm">Searching…</p>
          <p v-else-if="results.length === 0" class="text-muted-foreground text-sm">
            No token printing matches that. Write one out instead.
          </p>
          <ul v-else class="grid grid-cols-[repeat(auto-fill,minmax(7rem,1fr))] gap-3">
            <li v-for="card in results" :key="card.id">
              <button
                type="button"
                class="hover:border-ring/60 w-full rounded-lg border p-1 text-left transition-colors"
                @click="createFromCard(card)"
              >
                <img
                  v-if="card.has_image"
                  :src="cardImageUrl(table.store.game, card.id, 'small')"
                  :alt="card.name"
                  loading="lazy"
                  class="aspect-[61/85] w-full rounded object-contain"
                />
                <span class="mt-1 block truncate text-xs">{{ card.name }}</span>
                <span class="text-muted-foreground block truncate text-[0.65rem]">
                  {{ card.type_line }}
                </span>
              </button>
            </li>
          </ul>
        </div>
      </template>

      <form v-else class="mt-3 grid gap-3 sm:grid-cols-2" @submit.prevent="createFromText">
        <label class="text-sm">
          <span class="mb-1 block">Name</span>
          <Input v-model="name" placeholder="Treasure" />
        </label>
        <label class="text-sm">
          <span class="mb-1 block">Type line</span>
          <Input v-model="typeLine" placeholder="Token Artifact" />
        </label>
        <label class="text-sm">
          <span class="mb-1 block">Power / toughness</span>
          <Input v-model="powerToughness" placeholder="1/1" />
        </label>
        <label class="text-sm">
          <span class="mb-1 block">Colours</span>
          <Input v-model="colors" placeholder="WUBRG" />
        </label>
        <div class="sm:col-span-2">
          <Button type="submit" :disabled="!name.trim()">Make it</Button>
        </div>
      </form>

      <div class="mt-4 flex justify-end">
        <Button variant="outline" @click="close">Cancel</Button>
      </div>
    </DialogContent>
  </Dialog>
</template>
