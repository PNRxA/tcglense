<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { Dices, Loader2, Redo2, Undo2 } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import CardImage from '@/components/cards/CardImage.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import UpdatingOverlay from '@/components/cards/UpdatingOverlay.vue'
import {
  useDeckGoldfishQuery,
  usePreconGoldfishQuery,
  usePublicDeckGoldfishQuery,
} from '@/composables/useDeckAnalysis'
import { useDetailModalLink } from '@/composables/useDetailModalLink'

// "Test hand" (issue #596): shuffle up, draw seven, mulligan, and step through draws — the
// thing every deckbuilder does a dozen times while tuning a list.
//
// The engine is the server's and it is stateless: a hand is a pure function of
// `(seed, mulligans, what was bottomed, how many drawn)`, so this component holds exactly
// those four values and the response is derived from them. That is what makes the seed
// worth showing — type it back in and you get the same hand, in a browser or from `curl`,
// which is what you want when a hand is worth reporting.
//
// `handle` puts it in public mode (a deck someone shared), like the analytics panel.
const props = defineProps<{
  game: string
  /** Addressing is fixed at mount and mutually exclusive: a deck id (the owner's own deck),
   *  a `handle` + deck id (a shared deck), or a `preconSlug` (a published catalog decklist,
   *  which has no deck row and no numeric id). Exactly one applies. */
  deckId?: number
  handle?: string
  preconSlug?: string
}>()

/** The opening hand size the format deals. */
const OPENING = 7

const seed = ref<number | null>(null)
const mulligans = ref(0)
const bottom = ref<string[]>([])
const draws = ref(0)

/** A fresh 32-bit seed — the range the API round-trips exactly through JSON. */
function rollSeed(): number {
  return Math.floor(Math.random() * 0x1_0000_0000)
}

function newHand() {
  seed.value = rollSeed()
  mulligans.value = 0
  bottom.value = []
  draws.value = 0
}

/** London: reshuffle, draw a full hand again, and owe one more card to the bottom. */
function mulligan() {
  mulligans.value += 1
  bottom.value = []
  draws.value = 0
}

function putOnBottom(cardId: string) {
  if (toBottom.value > 0) bottom.value = [...bottom.value, cardId]
}

function undoBottom() {
  bottom.value = bottom.value.slice(0, -1)
}

function draw() {
  draws.value += 1
}

const params = computed(() => ({
  seed: seed.value ?? undefined,
  mulligans: mulligans.value,
  bottom: bottom.value,
  draws: draws.value,
  opening: OPENING,
}))

const game = toRef(props, 'game')
const deckId = computed(() => props.deckId ?? 0)
const handle = computed(() => props.handle ?? '')
const preconSlug = computed(() => props.preconSlug ?? '')
const enabled = computed(() => seed.value !== null)
// Three addressing modes, selected ONCE at mount — see the props.
const handQuery = props.preconSlug
  ? usePreconGoldfishQuery(game, preconSlug, params, enabled)
  : props.handle
    ? usePublicDeckGoldfishQuery(handle, deckId, params, enabled)
    : useDeckGoldfishQuery(game, deckId, params, enabled)
const hand = computed(() => (seed.value === null ? undefined : handQuery.data.value))
// A refetch really can fail: edit the deck after bottoming a card and the invalidated query
// re-asks with a `bottom` that is no longer in the reshuffled hand, which is a 422 the retry
// policy won't retry. Without this the panel would just empty itself with nothing said.
const handError = computed(() => (seed.value === null ? null : handQuery.error.value))

// Every button here is a round trip — the shuffle is the server's (issue #596) — so the
// panel has to show one in flight or a click reads as a dead button.
//
// Two shapes, because there are two situations. `dealing` is the first hand of a run: there
// is nothing on screen to keep, so the seven frames are drawn as skeletons and the card
// stops collapsing to a bare header while the deal is in the air. `updating` is a mulligan,
// a draw or a bottom, where keepPreviousData deliberately holds the hand already on screen —
// which is exactly why it needs a cue of its own: the visible hand is the *previous* one.
//
// Stepping back to a hand already in the cache stays instant (the goldfish is a pure
// function of its parameters, so `staleTime: Infinity`) — no fetch, so neither flag trips
// and nothing flickers.
const fetching = computed(() => seed.value !== null && handQuery.isFetching.value)
const dealing = computed(() => fetching.value && !hand.value)
const updating = computed(() => fetching.value && !!hand.value)

const toBottom = computed(() => hand.value?.to_bottom ?? 0)
const libraryLeft = computed(() => hand.value?.library_size ?? 0)
const cards = computed(() => hand.value?.hand ?? [])
/** Cards past the opening hand were drawn this game — worth marking, so a new draw is
 * visible without re-reading the whole hand. */
const firstDrawnIndex = computed(() => (hand.value ? hand.value.hand.length - hand.value.draws : 0))
const keepLabel = computed(() => `Mulligan to ${Math.max(0, OPENING - mulligans.value - 1)}`)

// A seed typed into the field replays that hand from the start; carrying a mulligan or a
// draw step across would apply an old decision to a different shuffle.
//
// The field keeps its own text state rather than being a computed over `seed`. A computed
// setter that early-returns on bad input moves nothing reactive, so Vue never re-renders and
// the box keeps displaying text that isn't the seed on screen. Here a rejected entry is
// snapped back by the watcher below, and blank is rejected explicitly — `Number('')` is 0,
// which would otherwise read as "replay seed 0" the moment someone cleared the box to paste.
const seedField = ref('')
watch(
  seed,
  (value) => {
    seedField.value = value === null ? '' : String(value)
  },
  { immediate: true },
)

function applySeed() {
  const text = seedField.value.trim()
  const parsed = Number(text)
  if (text === '' || !Number.isInteger(parsed) || parsed < 0 || parsed > 0xffff_ffff) {
    seedField.value = seed.value === null ? '' : String(seed.value)
    return
  }
  if (parsed === seed.value) return
  seed.value = parsed
  mulligans.value = 0
  bottom.value = []
  draws.value = 0
}

// A card in hand is a card like any other on the deck page: a plain click opens it in the
// shared detail modal over this panel — the hand, its seed and its bottom decisions stay put
// underneath, and Back closes it — while the href stays the real card page so modifier and
// middle clicks, "open in new tab", and crawlers get the full document. The same contract
// DeckCardRow and CardTile keep, through the same seam. The one time a click means something
// else is the London bottom: while cards are still owed to the bottom, clicking one bottoms
// it (the button below), and the link waits until that decision is made — a hand mid-mulligan
// must not open a modal when the prompt above it says "click one in your hand".
const { hrefFor, onActivate, warm } = useDetailModalLink()

// Editing the deck invalidates the hand: it was dealt from a library that no longer exists.
// The hand was dealt from a library that no longer applies — whether the deck was edited or
// the route swapped the subject entirely. vue-router reuses the precon page across `:slug`,
// so watching only `deckId` would carry a seed (and a `bottom` naming cards that aren't in
// the new deck, which is a 422) into a different decklist.
watch(
  () => [props.deckId, props.preconSlug],
  () => {
    seed.value = null
  },
)
</script>

<template>
  <Card class="mb-6">
    <CardHeader class="flex flex-row flex-wrap items-center justify-between gap-3 space-y-0">
      <CardTitle class="text-base">Test hand</CardTitle>
      <div class="flex flex-wrap items-center gap-2">
        <label v-if="hand" class="text-muted-foreground flex items-center gap-1.5 text-xs">
          Seed
          <input
            v-model="seedField"
            type="text"
            inputmode="numeric"
            class="border-input bg-background w-28 rounded-md border px-2 py-1 text-xs tabular-nums"
            aria-label="Shuffle seed — type one to replay that hand"
            @change="applySeed"
            @keyup.enter="applySeed"
          />
        </label>
        <!-- The spinner sits in the button that was clicked, which is where the eye already
          is; the label stays put so the control doesn't resize under the cursor. -->
        <Button size="sm" variant="outline" :disabled="fetching" @click="newHand">
          <component
            :is="fetching ? Loader2 : Dices"
            class="size-4"
            :class="{ 'animate-spin': fetching }"
            aria-hidden="true"
          />
          {{ hand ? 'New hand' : 'Draw opening hand' }}
        </Button>
      </div>
    </CardHeader>

    <CardContent v-if="handError" class="space-y-3">
      <p class="text-destructive text-sm" aria-live="polite">
        {{ handError.message || 'That hand could not be dealt.' }}
      </p>
      <p class="text-muted-foreground text-sm">
        The deck may have changed since it was shuffled — draw a new hand.
      </p>
    </CardContent>

    <!-- The opening deal: no hand to hold on to, so the frames it is about to fill are drawn
      instead. Without this the card collapsed to its header for the whole round trip. -->
    <CardContent v-else-if="dealing" class="space-y-4" aria-busy="true">
      <p class="text-muted-foreground text-sm"><UpdatingCue label="Shuffling up…" /></p>
      <ul class="grid grid-cols-3 gap-2 sm:grid-cols-5 lg:grid-cols-7">
        <li v-for="slot in OPENING" :key="slot">
          <Skeleton class="aspect-[61/85] w-full rounded-lg" />
        </li>
      </ul>
    </CardContent>

    <CardContent v-else-if="hand" class="space-y-4" :aria-busy="updating || undefined">
      <div class="flex flex-wrap items-center gap-2">
        <Button size="sm" variant="secondary" :disabled="mulligans >= OPENING" @click="mulligan">
          {{ keepLabel }}
        </Button>
        <Button
          size="sm"
          variant="secondary"
          :disabled="toBottom > 0 || libraryLeft === 0"
          @click="draw"
        >
          <Redo2 class="size-4" aria-hidden="true" />
          Draw
        </Button>
        <Button v-if="bottom.length" size="sm" variant="ghost" @click="undoBottom">
          <Undo2 class="size-4" aria-hidden="true" />
          Undo bottom
        </Button>
        <!-- These counts describe the hand on screen, which during a fetch is the one
          *before* the click — so say "dealing" rather than print a hand size that is about
          to change. -->
        <p class="text-muted-foreground text-xs" aria-live="polite">
          <template v-if="updating"><UpdatingCue label="Dealing…" /></template>
          <template v-else>
            {{ cards.length }} in hand · {{ libraryLeft }} in library
            <template v-if="hand.draws"> · {{ hand.draws }} drawn</template>
          </template>
        </p>
      </div>

      <p v-if="toBottom > 0" class="text-warning text-sm">
        Put {{ toBottom }} card{{ toBottom === 1 ? '' : 's' }} on the bottom — click
        {{ toBottom === 1 ? 'one' : 'them' }} in your hand.
      </p>

      <!-- The hand the click is replacing stays put and dims, the way a paged grid does.
        The overlay's `inert` matters here beyond the visual: while a bottom is in flight the
        cards on screen are the pre-bottom hand, so a second click on the same card would
        send a duplicate id the server rejects (422). -->
      <UpdatingOverlay :loading="updating">
        <ul v-if="cards.length" class="grid grid-cols-3 gap-2 sm:grid-cols-5 lg:grid-cols-7">
          <li v-for="(card, index) in cards" :key="`${card.id}-${index}`" class="relative">
            <!-- Two elements rather than one `<component :is>`: what a click *does* differs
              (bottom the card vs open it), so the handlers, the accessible name, and the
              element's own semantics — a button acts, a link goes somewhere — differ with it. -->
            <button
              v-if="toBottom > 0"
              type="button"
              class="focus-visible:ring-ring block w-full cursor-pointer rounded-lg text-left focus-visible:ring-2 focus-visible:outline-none"
              :aria-label="`Put ${card.name} on the bottom`"
              @click="putOnBottom(card.id)"
            >
              <CardImage
                :game="game"
                :id="card.id"
                :name="card.name"
                size="normal"
                :has-image="card.has_image"
                class="rounded-lg transition hover:brightness-110"
              />
            </button>
            <!-- The image's alt (or the no-image frame's text) is the card's name, which is
              the link's accessible name; `title` repeats it for the hover the grid doesn't
              otherwise label, since a hand shows art only. -->
            <a
              v-else
              :href="hrefFor('card', game, card.id)"
              class="focus-visible:ring-ring block w-full rounded-lg focus-visible:ring-2 focus-visible:outline-none"
              :title="card.name"
              @click="onActivate($event, 'card', game, card.id)"
              @pointerenter="warm('card')"
              @focusin="warm('card')"
            >
              <CardImage
                :game="game"
                :id="card.id"
                :name="card.name"
                size="normal"
                :has-image="card.has_image"
                class="rounded-lg transition hover:brightness-110"
              />
            </a>
            <!-- `pointer-events-none`: the badge sits over the card's corner, and a tap on it
              is a tap on the card — it must fall through to the link or button beneath. -->
            <span
              v-if="index >= firstDrawnIndex"
              class="bg-primary text-primary-foreground pointer-events-none absolute top-1 left-1 rounded px-1.5 py-0.5 text-[0.65rem] font-medium"
            >
              drawn
            </span>
          </li>
        </ul>
        <p v-else class="text-muted-foreground text-sm">
          This deck has no cards in the sections a hand is dealt from.
        </p>
      </UpdatingOverlay>

      <p v-if="hand.bottomed.length" class="text-muted-foreground text-xs">
        On the bottom: {{ hand.bottomed.map((card) => card.name).join(', ') }}
      </p>
    </CardContent>
  </Card>
</template>
