<script setup lang="ts">
import { computed, ref, toRef, useId, watch } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw, type LocationQueryValue } from 'vue-router'
import { Loader2 } from '@lucide/vue'
import CardImage from '@/components/cards/CardImage.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { ApiError, MAX_OPENING_COPIES, MAX_OPENING_PACKS, type Product } from '@/lib/api'
import { usePackOpeningQuery, useProductEvQuery } from '@/composables/useProducts'
import { useCurrency } from '@/composables/useCurrency'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import { PRODUCT_OPENER_KEYS, type PackOpenerKeys } from '@/composables/useProductCardsSearch'
import { boosterLabel, openingSummary, openingVersusPrice } from '@/lib/productCounts'

// "Open a pack": a seeded simulation of opening this product (issue #682), dealt by the API
// (`GET /api/games/{game}/products/{id}/open?seed=&copies=`) off the same booster sheets the
// expected-value panel above is computed from.
//
// The opening is a **pure function of its URL** server-side, and this panel keeps that
// property client-side: the seed is minted here (a u32 from `crypto.getRandomValues`),
// mirrored into the route as `?pack=<seed>&copies=<n>` (the `keys` prop names the pair — the
// modal namespaces them), and read back on mount *and on every product change* — so a run can
// be linked, replayed, and argued about, and re-rendering it never costs a request
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
const props = withDefaults(
  defineProps<{
    game: string
    id: string
    // For the "vs what a copy costs" line only; the panel never waits on it.
    product?: Product
    // The URL keys the seed + copies ride. The full page owns its route and keeps the plain
    // pair (the shareable contract); the detail modal passes namespaced keys, because a seed
    // left behind in a *browse* URL is not inert — the next product opened would read it and
    // deal an opening nobody asked for. Fixed per mount, like the card list's `searchKeys`.
    keys?: PackOpenerKeys
  }>(),
  { product: undefined, keys: () => PRODUCT_OPENER_KEYS },
)
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
function seedFromRoute(): number | null {
  return parseIntInRange(firstQuery(route.query[props.keys.pack]), 0, MAX_SEED)
}
function copiesFromRoute(): number {
  return parseIntInRange(firstQuery(route.query[props.keys.copies]), 1, MAX_OPENING_COPIES) ?? 1
}
const seed = ref<number | null>(seedFromRoute())
const copies = ref(copiesFromRoute())

// A different product is a different run: re-read both from wherever we landed, exactly as
// `useProductCardsSearch` resyncs its box on `id`. Keyed on the id, never `route.path` — the
// detail modal steps products by rewriting only `?product=`, so a path watch never fires
// there and the panel would carry one product's seed onto the next, dealing an opening
// nobody asked for (issue #448's failure mode, with a request attached).
watch(id, () => {
  seed.value = seedFromRoute()
  copies.value = copiesFromRoute()
})

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
// Two in-flight states, split the way `DeckGoldfish` splits its deal from its mulligan.
// `dealing` is the first open: nothing on screen to keep, so the rows the run is about to
// fill are drawn as skeletons rather than one line of text under an empty panel. `updating`
// is "Open another": `keepPreviousData` deliberately holds the last run's cards up so the
// panel doesn't collapse — which is exactly why it needs a cue of its own, since everything
// visible is the *previous* roll. Both disable the buttons: a second click mid-deal mints a
// second seed and cancels the run the URL already names.
const fetching = computed(() => seed.value !== null && openQuery.isFetching.value)
const dealing = computed(() => fetching.value && !opening.value)
const updating = computed(() => fetching.value && !!opening.value)
/** How many card rows the first deal draws as skeletons: the pack's own expected count,
 * capped so a box doesn't sketch 500 placeholders for a list that scrolls anyway. */
const SKELETON_ROW_CAP = 15
const skeletonRows = computed(() => {
  const perPack = ev.value?.packs?.[0]?.cards_per_pack ?? 0
  return Math.min(SKELETON_ROW_CAP, Math.max(6, Math.ceil(perPack)))
})

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

// The copies control only renders where a copy IS a pack, so a value carried in from a URL (or
// from the product before this one) must be stood back down once we know it isn't: six copies
// of a booster box is 216 packs, a guaranteed 422 with no visible control to undo it. Gated on
// the EV answer having landed — `packsPerCopy` is 0 while it's in flight, and clamping off that
// would throw away a legitimately deep-linked `?copies=` before the panel knows anything.
watch(
  () => (ev.value === null ? null : singlePack.value),
  (single) => {
    if (single === false) copies.value = 1
  },
  { immediate: true },
)

// Mirror the run into the URL so it can be shared and replayed. `replace`, not `push`: a
// re-roll is not a place in the history to go Back to, and the page underneath keeps its
// scroll and its other query state (the product modal's `?product=` included).
watch([seed, copies], () => {
  if (seed.value === null) return
  const pack = String(seed.value)
  const copiesValue = copies.value > 1 ? String(copies.value) : undefined
  // Nothing to write when the URL already says this — an id-resync that adopted a deep link's
  // own values would otherwise `replace` to an identical URL.
  const currentPack = firstQuery(route.query[props.keys.pack])
  const currentCopies = firstQuery(route.query[props.keys.copies]) ?? undefined
  if (currentPack === pack && currentCopies === copiesValue) return
  const query: LocationQueryRaw = { ...route.query, [props.keys.pack]: pack }
  if (copiesValue) query[props.keys.copies] = copiesValue
  else delete query[props.keys.copies]
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
      <Button type="button" :disabled="fetching" @click="openAnother">
        <Loader2 v-if="fetching" class="size-4 animate-spin" aria-hidden="true" />
        {{ fetching ? 'Dealing…' : openLabel }}
      </Button>

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
    <div aria-live="polite" class="mt-4" :aria-busy="fetching || undefined">
      <p v-if="failure" class="text-muted-foreground text-sm">{{ failure }}</p>
      <p v-else-if="fetching" class="text-muted-foreground text-sm">
        <UpdatingCue label="Dealing…" />
      </p>
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

    <!-- The first deal: no run to hold on to, so the rows it is about to fill are drawn
      instead. Without this the panel was one word of text for the whole round trip. -->
    <div v-if="dealing" class="mt-4" aria-hidden="true">
      <div class="flex items-baseline justify-between gap-3">
        <Skeleton class="h-4 w-40" />
        <Skeleton class="h-4 w-12" />
      </div>
      <ul class="mt-1.5 grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
        <li v-for="row in skeletonRows" :key="row">
          <div class="flex items-center gap-2 rounded-lg border p-2">
            <Skeleton class="aspect-[61/85] w-9 shrink-0 rounded" />
            <div class="min-w-0 flex-1 space-y-1.5">
              <Skeleton class="h-3.5 w-3/4" />
              <Skeleton class="h-3 w-1/3" />
            </div>
            <Skeleton class="h-3 w-10 shrink-0" />
          </div>
        </li>
      </ul>
    </div>

    <template v-if="opening && !failure">
      <!-- One block per pack, in the order the API dealt them. While the next run is in the
        air these are the PREVIOUS roll's cards — dimmed so they can't be read as the new one,
        and inert for the same reason. -->
      <div
        class="mt-4 space-y-4 transition-opacity"
        :class="{ 'pointer-events-none opacity-40': updating }"
        :aria-busy="updating || undefined"
      >
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
        <Button type="button" variant="outline" :disabled="fetching" @click="openAnother">
          <Loader2 v-if="updating" class="size-4 animate-spin" aria-hidden="true" />
          {{ updating ? 'Dealing…' : 'Open another' }}
        </Button>
      </div>

      <!-- The server's own caveats, verbatim: what the simulation does and doesn't model. -->
      <ul v-if="opening.caveats.length" class="text-muted-foreground mt-3 space-y-1 text-xs">
        <li v-for="(caveat, index) in opening.caveats" :key="index">{{ caveat }}</li>
      </ul>
    </template>
  </section>
</template>
