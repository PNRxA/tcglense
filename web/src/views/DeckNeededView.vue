<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink, useRoute } from 'vue-router'
import { ArrowLeft, Heart, Layers, ShoppingCart } from '@lucide/vue'
import { Button, buttonVariants } from '@/components/ui/button'
import CardTile from '@/components/cards/CardTile.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import StaleNotice from '@/components/cards/StaleNotice.vue'
import { useGamesQuery } from '@/composables/useCatalog'
import { useCurrency } from '@/composables/useCurrency'
import { useNeededCardsQuery } from '@/composables/useDecks'
import { useNeededWishlist } from '@/composables/useNeededWishlist'
import type { NeedMode, NeededCard } from '@/lib/api'
import { neededCostLine, neededEntryPrice } from '@/lib/neededCost'
import { useAuthStore } from '@/stores/auth'
import { usePageMeta } from '@/lib/seo'

const props = defineProps<{ game: string }>()
const game = computed(() => props.game)
const auth = useAuthStore()
const money = useCurrency()
const route = useRoute()

const { data: games } = useGamesQuery()
const gameName = computed(
  () => games.value?.data.find((g) => g.id === props.game)?.name ?? props.game.toUpperCase(),
)

// `?deck=<id>` scopes the list to one deck (issue #675): the deck page's "to finish" line
// links here with it. Anything that isn't a positive integer is the game-wide list.
const deckId = computed<number | null>(() => {
  const raw = route.query.deck
  const value = Number(Array.isArray(raw) ? raw[0] : raw)
  return Number.isInteger(value) && value > 0 ? value : null
})

// The two matching modes (issue #499): by gameplay card across any printing (the default),
// or by the exact missing printing. A ref inside the query key, so switching refetches.
const mode = ref<NeedMode>('card')
const MODES: { value: NeedMode; label: string; hint: string }[] = [
  {
    value: 'card',
    label: 'By card',
    hint: 'Any printing you own covers any printing your decks want.',
  },
  {
    value: 'printing',
    label: 'Exact printing',
    hint: 'Match each deck’s exact printing against that printing in your collection.',
  },
]
const activeHint = computed(() => MODES.find((m) => m.value === mode.value)?.hint ?? '')

const neededQuery = useNeededCardsQuery(game, mode, deckId)
const needed = computed(() => neededQuery.data.value?.data ?? [])
const scopedDeck = computed(() => neededQuery.data.value?.deck ?? null)
const totals = computed(() => neededQuery.data.value?.totals ?? null)
const summaryLine = computed(() =>
  totals.value ? neededCostLine(totals.value, money.formatUsd) : '',
)
// A scoped id that isn't one of the caller's decks is a 404 — worded as such, not as an
// outage.
const scopedMiss = computed(
  () =>
    deckId.value !== null &&
    neededQuery.isLoadingError.value &&
    neededQuery.error.value?.status === 404,
)

const title = computed(() =>
  scopedDeck.value ? `Cards needed for ${scopedDeck.value.name}` : 'Cards needed',
)
usePageMeta({ title: computed(() => `${title.value} — ${gameName.value} decks`), noindex: true })

// "Add all to wish list": top the wish list up to the copies still needed, once.
const {
  ready: wishlistReady,
  toAdd,
  pending: adding,
  result: added,
  error: addError,
  addAll,
} = useNeededWishlist(game, needed)
/** The badge's explanation, worded for the scope: game-wide the three numbers add up;
 *  scoped they need not, since the copies you own may be spoken for by another deck. */
function needTitle(entry: NeededCard): string {
  if (deckId.value === null) {
    return `You need ${entry.needed} more (your decks want ${entry.required}, you own ${entry.owned})`
  }
  const sharedAway = entry.needed > entry.required - entry.owned
  return sharedAway
    ? `You need ${entry.needed} more for this deck: it wants ${entry.required} and you own ${entry.owned}, but your other decks want those copies too`
    : `You need ${entry.needed} more for this deck (it wants ${entry.required}, you own ${entry.owned})`
}

const addLabel = computed(() => {
  if (adding.value) return 'Adding…'
  if (!wishlistReady.value) return 'Add to wish list'
  if (toAdd.value === 0) return 'On your wish list'
  return `Add ${toAdd.value} to wish list`
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-8">
    <!-- Signed-out: prompt in place rather than bouncing to /login. -->
    <div
      v-if="auth.sessionResolved && !auth.isAuthenticated"
      class="mx-auto max-w-md py-16 text-center"
    >
      <div class="bg-muted mx-auto flex size-12 items-center justify-center rounded-lg">
        <Layers class="size-6" aria-hidden="true" />
      </div>
      <h1 class="mt-4 text-xl font-semibold">Sign in to see what your decks need</h1>
      <p class="text-muted-foreground mt-2">
        This compares every {{ gameName }} deck against your collection to show the cards you still
        need. Sign in to view it.
      </p>
      <div class="mt-6 flex justify-center gap-3">
        <RouterLink
          :class="buttonVariants()"
          :to="{ path: '/login', query: { redirect: route.fullPath } }"
          >Sign in</RouterLink
        >
      </div>
    </div>

    <template v-else>
      <RouterLink
        :to="deckId !== null ? `/decks/${game}/${deckId}` : `/decks/${game}`"
        class="text-muted-foreground hover:text-foreground mb-3 inline-flex items-center gap-1 text-sm"
      >
        <ArrowLeft class="size-4" /> {{ deckId !== null ? 'Back to deck' : 'All decks' }}
      </RouterLink>

      <header class="mb-5 flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 class="flex items-center gap-2 text-2xl font-semibold tracking-tight">
            <ShoppingCart class="size-6" aria-hidden="true" /> {{ title }}
          </h1>
          <p class="text-muted-foreground mt-1 text-sm">
            <template v-if="deckId !== null">
              Cards this deck still wants beyond what your collection holds — its share of what
              every deck wants, so a copy two decks run is only counted once.
              <RouterLink :to="`/decks/${game}/needed`" class="hover:text-foreground underline">
                Every deck’s list
              </RouterLink>
            </template>
            <template v-else>
              Cards your {{ gameName }} decks want beyond what your collection holds.
            </template>
          </p>
        </div>

        <!-- Mode toggle: by card (any printing) vs exact printing. -->
        <div class="inline-flex rounded-lg border p-0.5" role="group" aria-label="Matching mode">
          <button
            v-for="m in MODES"
            :key="m.value"
            type="button"
            class="rounded-md px-3 py-1 text-sm transition"
            :class="
              mode === m.value
                ? 'bg-primary text-primary-foreground'
                : 'text-muted-foreground hover:text-foreground'
            "
            :aria-pressed="mode === m.value"
            @click="mode = m.value"
          >
            {{ m.label }}
          </button>
        </div>
      </header>
      <p class="text-muted-foreground -mt-3 mb-5 text-xs">{{ activeHint }}</p>

      <StaleNotice
        v-if="neededQuery.isRefetchError.value"
        label="Couldn't refresh — showing the last list worked out."
      />

      <LoadingRow v-if="neededQuery.isPending.value" label="Checking your decks…" />
      <p v-else-if="scopedMiss" class="text-muted-foreground py-16 text-center">
        That deck isn't one of yours.
        <RouterLink :to="`/decks/${game}/needed`" class="hover:text-foreground underline">
          See every deck’s list
        </RouterLink>
      </p>
      <p v-else-if="neededQuery.isLoadingError.value" class="text-destructive py-8">
        Couldn't work out what your decks need. Please retry.
      </p>
      <p v-else-if="needed.length === 0" class="text-muted-foreground py-16 text-center">
        <template v-if="deckId !== null">
          Your collection already covers every card in this deck. Nothing to buy!
        </template>
        <template v-else>
          Your collection already covers every card across your {{ gameName }} decks. Nothing to
          buy!
        </template>
      </p>

      <template v-else>
        <!-- The size and the money (issue #675), worded by the same seam as the deck page's
          "to finish" line, beside the one action a shopping list wants: onto the wish list. -->
        <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
          <p class="text-muted-foreground text-sm tabular-nums">{{ summaryLine }}</p>
          <div class="flex flex-wrap items-center gap-2">
            <p v-if="added" class="text-muted-foreground text-sm" aria-live="polite">
              Added {{ added.added }} to your
              <RouterLink :to="`/wishlist/${game}`" class="hover:text-foreground underline"
                >wish list</RouterLink
              >.
              <span v-if="added.failed > 0" class="text-destructive">
                {{ added.failed }} couldn't be added.</span
              >
            </p>
            <p v-else-if="addError" class="text-destructive text-sm" aria-live="polite">
              {{ addError.message }}
            </p>
            <Button
              variant="outline"
              size="sm"
              :disabled="adding || !wishlistReady || toAdd === 0"
              data-testid="add-all-wishlist"
              @click="addAll()"
            >
              <Heart class="size-4" aria-hidden="true" /> {{ addLabel }}
            </Button>
          </div>
        </div>
        <div class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
          <div v-for="entry in needed" :key="entry.card.id">
            <CardTile :game="game" :card="entry.card">
              <template #badge>
                <span
                  class="bg-primary text-primary-foreground absolute top-1.5 right-1.5 z-20 inline-flex cursor-default items-center gap-0.5 rounded-md px-1.5 py-0.5 text-xs font-medium shadow select-none"
                  :title="needTitle(entry)"
                >
                  need {{ entry.needed }}
                </span>
              </template>
            </CardTile>
            <!-- Scoped, `required` is this deck's and `owned` is the collection's, and the
              shortfall is this deck's share of what every deck wants — so "wants 1 · own 1"
              can honestly sit under "need 1" when another deck runs the same copy. -->
            <p class="text-muted-foreground mt-1 text-xs tabular-nums">
              <template v-if="deckId !== null">this deck wants {{ entry.required }}</template>
              <template v-else>want {{ entry.required }}</template>
              · own {{ entry.owned }}
            </p>
            <!-- What the missing copies cost as held, then at the cheapest printing when
              that's less; an unpriced card says nothing rather than $0. -->
            <p
              v-if="neededEntryPrice(entry, money.formatUsd)"
              class="text-muted-foreground mt-0.5 text-xs tabular-nums"
              :title="`The ${entry.needed} missing ${entry.needed === 1 ? 'copy' : 'copies'} at the printing your decks run, then at the cheapest printing`"
            >
              {{ neededEntryPrice(entry, money.formatUsd) }}
            </p>
            <!-- Which decks want this card (issue #499). -->
            <div class="mt-1 flex flex-wrap gap-1">
              <RouterLink
                v-for="deck in entry.decks"
                :key="deck.id"
                :to="`/decks/${game}/${deck.id}`"
                class="bg-muted text-muted-foreground hover:text-foreground max-w-full truncate rounded px-1.5 py-0.5 text-xs"
                :title="deck.name"
              >
                {{ deck.name }}
              </RouterLink>
            </div>
          </div>
        </div>
      </template>
    </template>
  </div>
</template>
