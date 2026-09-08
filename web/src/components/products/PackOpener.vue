<script setup lang="ts">
import { computed, ref, toRef, useId, watch } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw, type LocationQueryValue } from 'vue-router'
import CardImage from '@/components/cards/CardImage.vue'
import { Button } from '@/components/ui/button'
import { ApiError, MAX_OPENING_COPIES, MAX_OPENING_PACKS, type Product } from '@/lib/api'
import { usePackOpeningQuery, useProductEvQuery } from '@/composables/useProducts'
import { useCurrency } from '@/composables/useCurrency'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import { boosterLabel, openingSummary, openingVersusPrice } from '@/lib/productCounts'

// "Open a pack": a seeded simulation of opening this product (issue #682), dealt by the API
// (`GET /api/games/{game}/products/{id}/open?seed=&copies=`) off the same booster sheets the
// expected-value panel above is computed from.
//
// The opening is a **pure function of its URL** server-side, and this panel keeps that
// property client-side: the seed is minted here (a u32 from `crypto.getRandomValues`),
// mirrored into the route as `?pack=<seed>&copies=<n>`, and read back on mount — so a run
// can be linked, replayed, and argued about, and re-rendering it never costs a request
// (`staleTime: Infinity`, a new seed being a new key rather than a refetch).
//
// The wording is the load-bearing part. A pulled total is the single most misleading number
// this page could print — one lucky roll read as what a box is worth — so every string
// around it comes from the `lib/productCounts.ts` vocabulary seam and stays in the past
// tense, about *this run*. The panel deliberately shows the pull's OWN price per card (a
// foil off a foil sheet is valued as a foil), which is why the cards render as a compact
// list rather than through `CardTile`: the tile prints the printing's regular price, which
// would contradict the run's own total on every foil pull.
//
// It self-hides for a product with no booster data — the same `['product-ev', …]` key the EV
// panel reads, so this costs no second fetch.
const props = defineProps<{
  game: string
  id: string
  // For the "vs what a copy costs" line only; the panel never waits on it.
  product?: Product
}>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')
const money = useCurrency()
const route = useRoute()
const router = useRouter()
const headingId = useId()

const evQuery = useProductEvQuery(game, id)
const ev = computed(() => evQuery.data.value?.data ?? null)
// How many packs one copy opens — the whole difference between "Open a pack" (a single
// booster) and "Open a copy" (a box, a bundle), and the ceiling the copies control obeys.
const packsPerCopy = computed(() =>
  (ev.value?.packs ?? []).reduce((sum, pack) => sum + Math.max(0, pack.quantity), 0),
)
const singlePack = computed(() => packsPerCopy.value === 1)
const openLabel = computed(() => (singlePack.value ? 'Open a pack' : 'Open a copy'))
const show = computed(() => ev.value !== null)

/** The first value of a repeated query key, or null — a hand-written `?pack=` can be
 * anything, so both the shape and the range are checked before either is trusted. */
function firstQuery(value: LocationQueryValue | LocationQueryValue[] | undefined): string | null {
  return Array.isArray(value) ? (value[0] ?? null) : (value ?? null)
}
function parseIntInRange(raw: string | null, min: number, max: number): number | null {
  if (!raw) return null
  const parsed = Number(raw)
  return Number.isInteger(parsed) && parsed >= min && parsed <= max ? parsed : null
}

/** The API's seed is a u32, so that is exactly what a shared link may carry. */
const MAX_SEED = 0xffffffff
const seed = ref<number | null>(parseIntInRange(firstQuery(route.query.pack), 0, MAX_SEED))
const copies = ref(parseIntInRange(firstQuery(route.query.copies), 1, MAX_OPENING_COPIES) ?? 1)

const openQuery = usePackOpeningQuery(game, id, seed, copies)
const opening = computed(() => openQuery.data.value ?? null)
const summary = computed(() => (opening.value ? openingSummary(opening.value) : null))
const versusPrice = computed(() =>
  opening.value ? openingVersusPrice(opening.value, props.product?.prices?.usd) : null,
)
// The API refuses an opening it can't deal (no booster data, zero copies, past its pack/card
// ceilings) with a 422 whose message says which — worth showing, since it is actionable.
// Anything else gets a short line: a failed simulation is not worth a stack trace.
const failure = computed(() => {
  const error = openQuery.error.value
  if (!error) return null
  return error instanceof ApiError && error.status === 422
    ? error.message
    : "That opening couldn't be dealt. Try again."
})
const dealing = computed(() => seed.value !== null && openQuery.isPending.value)

/** A fresh u32. `crypto.getRandomValues` is the source; the fallback only matters in an
 * environment without it, where a repeated seed costs nothing but a repeated run. */
function mintSeed(): number {
  const random = globalThis.crypto?.getRandomValues?.(new Uint32Array(1))?.[0]
  return random ?? Math.floor(Math.random() * (MAX_SEED + 1))
}
function openAnother() {
  seed.value = mintSeed()
}

/** How many copies this option would open, against the API's per-request pack ceiling. */
const copyOptions = computed(() =>
  Array.from({ length: MAX_OPENING_COPIES }, (_, index) => index + 1).map((count) => ({
    count,
    over: count * Math.max(1, packsPerCopy.value) > MAX_OPENING_PACKS,
  })),
)

// Mirror the run into the URL so it can be shared and replayed. `replace`, not `push`: a
// re-roll is not a place in the history to go Back to, and the page underneath keeps its
// scroll and its other query state (the product modal's `?product=` included).
watch([seed, copies], () => {
  if (seed.value === null) return
  const query: LocationQueryRaw = { ...route.query, pack: String(seed.value) }
  if (copies.value > 1) query.copies = String(copies.value)
  else delete query.copies
  void router.replace({ query })
})

const { hrefFor, onActivate, warm } = useDetailModalLink()
</script>

<template>
  <section v-if="show" :aria-labelledby="headingId">
    <h2 :id="headingId" class="mb-1 text-base font-semibold tracking-tight">{{ openLabel }}</h2>
    <p class="text-muted-foreground mb-3 text-xs">
      A simulated opening off this product's booster sheets — seeded, so the link reproduces it
      exactly.
    </p>

    <div class="flex flex-wrap items-center gap-3">
      <Button type="button" @click="openAnother">{{ openLabel }}</Button>

      <!-- Only where a copy IS a pack: opening six boxes is not a control anyone wants, and
        the API's own pack ceiling would refuse most of them anyway. -->
      <label v-if="singlePack" class="text-muted-foreground flex items-center gap-2 text-xs">
        Copies
        <select
          v-model.number="copies"
          class="bg-background rounded-md border px-2 py-1 text-sm"
          aria-label="Copies to open"
        >
          <option
            v-for="option in copyOptions"
            :key="option.count"
            :value="option.count"
            :disabled="option.over"
          >
            {{ option.count }}
          </option>
        </select>
        <span class="hidden sm:inline">6 for a sealed pool</span>
      </label>
    </div>

    <!-- Everything the run produced. Polite, because it lands on a button press: the totals
      are what a screen-reader user is waiting to hear, and the card list below them is long. -->
    <div aria-live="polite" class="mt-4">
      <p v-if="failure" class="text-muted-foreground text-sm">{{ failure }}</p>
      <p v-else-if="dealing" class="text-muted-foreground text-sm">Dealing…</p>
      <template v-else-if="opening && summary">
        <div class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <span class="text-2xl font-semibold tabular-nums">{{
            money.formatUsd(opening.value_usd)
          }}</span>
          <span class="text-muted-foreground text-sm">{{ summary.value }}</span>
        </div>
        <h3 class="mt-1 text-sm font-medium">{{ summary.title }}</h3>
        <p class="text-muted-foreground text-xs">{{ summary.blurb }}</p>
        <p v-if="versusPrice" class="text-muted-foreground text-xs">{{ versusPrice }}</p>
      </template>
    </div>

    <template v-if="opening && !failure">
      <!-- One block per pack, in the order the API dealt them. -->
      <div class="mt-4 space-y-4">
        <div v-for="(pack, index) in opening.packs" :key="index">
          <div class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
            <h3 class="text-sm font-medium">
              Pack {{ index + 1 }}
              <span class="text-muted-foreground font-normal">· {{ boosterLabel(pack) }}</span>
            </h3>
            <span class="text-sm font-semibold tabular-nums">{{
              money.formatUsd(pack.value_usd)
            }}</span>
          </div>
          <ul class="mt-1.5 grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
            <li v-for="(pull, pullIndex) in pack.cards" :key="pullIndex">
              <a
                :href="hrefFor('card', game, pull.card.id)"
                class="hover:bg-muted/50 flex items-center gap-2 rounded-lg border p-2 transition-colors"
                @click="onActivate($event, 'card', game, pull.card.id)"
                @pointerenter="warm('card')"
                @focusin="warm('card')"
              >
                <div class="w-9 shrink-0">
                  <CardImage
                    :game="game"
                    :id="pull.card.id"
                    :name="pull.card.name"
                    :has-image="pull.card.has_image"
                    size="small"
                  />
                </div>
                <div class="min-w-0 flex-1">
                  <p class="truncate text-sm font-medium">{{ pull.card.name }}</p>
                  <p class="text-muted-foreground flex items-center gap-1.5 text-xs">
                    <span class="truncate">{{ pull.sheet }}</span>
                    <span
                      v-if="pull.foil"
                      class="bg-foil/15 text-foil shrink-0 rounded px-1 py-0.5 text-[0.65rem] font-semibold tracking-wide uppercase"
                      >Foil</span
                    >
                  </p>
                </div>
                <!-- The price this pull was valued at (its foil price off a foil sheet), so
                  the card list and the run's total can never disagree. -->
                <span class="shrink-0 text-xs tabular-nums">{{
                  money.formatUsd(pull.price_usd) ?? '—'
                }}</span>
              </a>
            </li>
          </ul>
        </div>
      </div>

      <div class="mt-4 flex flex-wrap items-center gap-3">
        <Button type="button" variant="outline" @click="openAnother">Open another</Button>
      </div>

      <!-- The server's own caveats, verbatim: what the simulation does and doesn't model. -->
      <ul v-if="opening.caveats.length" class="text-muted-foreground mt-3 space-y-1 text-xs">
        <li v-for="(caveat, index) in opening.caveats" :key="index">{{ caveat }}</li>
      </ul>
    </template>
  </section>
</template>
