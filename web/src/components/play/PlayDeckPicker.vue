<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { Check, Loader2, Search } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import { useDecksQuery } from '@/composables/useDecks'
import { usePreconsQuery } from '@/composables/usePrecons'
import { useLoadPlaySeatDeck } from '@/composables/usePlayRooms'
import type { LoadPlayDeckBody } from '@/lib/api/play'
import { useAuthStore } from '@/stores/auth'

// What a seat plays, from the three places a deck can come from.
//
// Three tabs rather than one clever box because the three sources answer different questions:
// "the deck I built here" (signed in only), "a precon, because we're playing precons" (anyone,
// and the reason a guest can play at all), and "the list I have in my clipboard" (the escape
// hatch that makes the tool useful before you've imported anything). The pasted list goes
// through the same `deck_import` text grammar as a deck import, so a Moxfield/Archidekt export
// pasted straight in works, and an unresolvable line comes back named rather than silently
// dropped — a 99-card deck that quietly loaded 97 is worse than one that refuses.
const props = defineProps<{
  game: string
  code: string
  seatId: number
  seatToken: string
  /** The deck currently in the seat, so the picker can say what it would replace. */
  currentDeckName?: string | null
}>()

const emit = defineEmits<{ loaded: [] }>()

const auth = useAuthStore()
const game = toRef(props, 'game')

type Tab = 'decks' | 'precons' | 'text'
// A guest has no decks of their own, so their first tab is the first one that can work.
const tab = ref<Tab>(auth.isAuthenticated ? 'decks' : 'precons')

const load = useLoadPlaySeatDeck()
/** The one thing being loaded, so only its row spins while the others stay clickable. */
const pending = ref<string | null>(null)

async function loadDeck(body: LoadPlayDeckBody, key: string) {
  pending.value = key
  try {
    await load.mutateAsync({
      game: props.game,
      code: props.code,
      seatId: props.seatId,
      seatToken: props.seatToken,
      body,
    })
    emit('loaded')
  } catch {
    // Rendered from `load.error` below — a deck that won't load is the picker's own business.
  } finally {
    pending.value = null
  }
}

// ---- My decks ----
const decks = useDecksQuery(game, () => tab.value === 'decks')
const myDecks = computed(() => decks.data.value?.data ?? [])

// ---- Precons ----
const search = ref('')
const preconPage = ref(1)
const noFilter = ref('')
const precons = usePreconsQuery(game, {
  page: preconPage,
  query: search,
  set: noFilter,
  type: noFilter,
  sort: noFilter,
  enabled: computed(() => tab.value === 'precons'),
})
const preconRows = computed(() => precons.data.value?.data ?? [])

// ---- Paste a list ----
const text = ref('')
const canLoadText = computed(() => text.value.trim().length > 0)

const TABS: Array<{ id: Tab; label: string }> = [
  { id: 'decks', label: 'My decks' },
  { id: 'precons', label: 'Precons' },
  { id: 'text', label: 'Paste a list' },
]
</script>

<template>
  <div class="bg-card rounded-xl border">
    <div class="flex items-center justify-between gap-3 border-b p-3">
      <div>
        <p class="text-sm font-medium">Your deck</p>
        <p class="text-muted-foreground mt-0.5 text-sm">
          <template v-if="currentDeckName">
            Playing <span class="text-foreground">{{ currentDeckName }}</span> — pick another to
            replace it.
          </template>
          <template v-else>Load a deck to take your seat at the table.</template>
        </p>
      </div>
      <Check v-if="currentDeckName" class="size-5 shrink-0 text-success" aria-hidden="true" />
    </div>

    <div class="flex gap-1 border-b p-2" role="tablist" aria-label="Where your deck comes from">
      <button
        v-for="entry in TABS"
        :key="entry.id"
        type="button"
        role="tab"
        :aria-selected="tab === entry.id"
        class="rounded-md px-3 py-1.5 text-sm font-medium transition-colors"
        :class="
          tab === entry.id
            ? 'bg-primary/15 text-primary'
            : 'text-muted-foreground hover:bg-accent hover:text-foreground'
        "
        @click="tab = entry.id"
      >
        {{ entry.label }}
      </button>
    </div>

    <div class="p-3">
      <!-- My decks -->
      <template v-if="tab === 'decks'">
        <p v-if="!auth.isAuthenticated" class="text-muted-foreground py-6 text-center text-sm">
          Sign in to play one of your own decks — or use a precon, or paste a list.
        </p>
        <LoadingRow v-else-if="decks.isPending.value" label="Loading your decks…" />
        <p v-else-if="!myDecks.length" class="text-muted-foreground py-6 text-center text-sm">
          You have no decks for this game yet.
        </p>
        <ul v-else class="max-h-72 space-y-1 overflow-y-auto">
          <li v-for="deck in myDecks" :key="deck.id">
            <button
              type="button"
              class="hover:bg-accent flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors"
              :disabled="pending !== null"
              @click="loadDeck({ source: 'deck', deck_id: deck.id }, `deck:${deck.id}`)"
            >
              <span class="min-w-0 flex-1">
                <span class="block truncate text-sm font-medium">{{ deck.name }}</span>
                <span class="text-muted-foreground block truncate text-xs">
                  {{ deck.card_count }} cards<template v-if="deck.commanders.length">
                    · {{ deck.commanders.map((c) => c.name).join(' + ') }}</template
                  >
                </span>
              </span>
              <Loader2
                v-if="pending === `deck:${deck.id}`"
                class="size-4 shrink-0 animate-spin"
                aria-hidden="true"
              />
            </button>
          </li>
        </ul>
      </template>

      <!-- Precons -->
      <template v-else-if="tab === 'precons'">
        <div class="relative">
          <Search class="text-muted-foreground absolute top-2.5 left-3 size-4" aria-hidden="true" />
          <Input
            v-model="search"
            class="pl-9"
            aria-label="Search preconstructed decks"
            placeholder="Search precons…"
          />
        </div>
        <LoadingRow v-if="precons.isPending.value" class="mt-2" label="Loading precons…" />
        <p v-else-if="!preconRows.length" class="text-muted-foreground py-6 text-center text-sm">
          No precons match that.
        </p>
        <ul v-else class="mt-2 max-h-72 space-y-1 overflow-y-auto">
          <li v-for="precon in preconRows" :key="precon.slug">
            <button
              type="button"
              class="hover:bg-accent flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors"
              :disabled="pending !== null"
              @click="loadDeck({ source: 'precon', slug: precon.slug }, `precon:${precon.slug}`)"
            >
              <span class="min-w-0 flex-1">
                <span class="block truncate text-sm font-medium">{{ precon.name }}</span>
                <span class="text-muted-foreground block truncate text-xs">
                  {{ precon.set_name ?? precon.set_code.toUpperCase() }} · {{ precon.deck_type }} ·
                  {{ precon.card_count }} cards
                </span>
              </span>
              <Loader2
                v-if="pending === `precon:${precon.slug}`"
                class="size-4 shrink-0 animate-spin"
                aria-hidden="true"
              />
            </button>
          </li>
        </ul>
      </template>

      <!-- Paste a list -->
      <template v-else>
        <Textarea
          v-model="text"
          rows="8"
          class="font-mono text-xs"
          aria-label="Paste a deck list"
          placeholder="1 Sol Ring&#10;1 Arcane Signet&#10;1 Atraxa, Praetors' Voice"
        />
        <p class="text-muted-foreground mt-2 text-xs">
          One card per line, with a count. A Moxfield or Archidekt text export works as-is; put your
          commander under a <code>Commander</code> heading.
        </p>
        <Button
          class="mt-2"
          size="sm"
          :disabled="!canLoadText || pending !== null"
          @click="loadDeck({ source: 'text', text }, 'text')"
        >
          <Loader2 v-if="pending === 'text'" class="size-4 animate-spin" aria-hidden="true" />
          Load list
        </Button>
      </template>

      <p v-if="load.error.value" class="text-destructive mt-3 text-sm">
        {{ load.error.value.message }}
      </p>
    </div>
  </div>
</template>
