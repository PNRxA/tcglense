<script setup lang="ts">
import { computed, toRef } from 'vue'
import { RouterLink } from 'vue-router'
import CardImageZoom from '@/components/cards/CardImageZoom.vue'
import CardMetaList from '@/components/cards/CardMetaList.vue'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'
import CardPriceSummary from '@/components/cards/CardPriceSummary.vue'
import CollectionControls from '@/components/collection/CollectionControls.vue'
import SetPriceAlertButton from '@/components/alerts/SetPriceAlertButton.vue'
import CardLegalities from '@/components/cards/CardLegalities.vue'
import CardPrints from '@/components/cards/CardPrints.vue'
import CardRulings from '@/components/cards/CardRulings.vue'
import CardSealedProducts from '@/components/products/CardSealedProducts.vue'
import CardBuyLinks from '@/components/cards/CardBuyLinks.vue'
import PriceChart from '@/components/cards/PriceChart.vue'
import { Skeleton } from '@/components/ui/skeleton'
import { useCardQuery } from '@/composables/useCatalog'
import { getPriceHistory, type AlertFinish } from '@/lib/api'
import { formatReleaseLabel } from '@/lib/releaseDate'

// The body of a card's detail — image(s), rules text, prices + history, collection and
// wish-list count controls, other printings — shared verbatim by the full page
// (CardDetailView) and the browse-grid modal (CardDetailDialog). Page chrome (meta
// tags, breadcrumb/back link, the modal frame) stays with the callers; both fetch the
// same ['card', game, id] key, so the page and an overlay never double-fetch.
//
// Layout: a full-width header (name, mana cost, type, at-a-glance chips) over a
// two-column body — a left rail holding the image plus everything price/ownership
// shaped (price tiles, collection + wish-list steppers, buy links), and a main column
// for the knowledge: rules text, details, price history, printings, sealed products.
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

const cardQuery = useCardQuery(game, id)

const card = computed(() => cardQuery.data.value)
// A future release date reads as "Releases …", a past one as "Released …", so an as-yet-unreleased
// (freshly-previewed) printing shows when it's due rather than claiming it already came out.
const releaseLabel = computed(() => formatReleaseLabel(card.value?.released_at))
// "Not found" once the fetch has settled without a card — not merely on `isError`: a 2xx
// with an empty body resolves to `undefined` data with `isError` false, which would
// otherwise sit on the loading skeleton forever. `!isPending` = settled, so a pending
// cache-miss still shows the skeleton below.
const notFound = computed(
  () => cardQuery.isError.value || (!card.value && !cardQuery.isPending.value),
)

// Layouts whose faces carry their OWN images (so we render one image per face).
// Split / flip / adventure / aftermath also have two faces, but only a single
// top-level image — those use the one-image path so we don't 404 per face.
const SEPARATE_FACE_IMAGE_LAYOUTS = [
  'transform',
  'modal_dfc',
  'double_faced_token',
  'reversible_card',
  'battle',
  'art_series',
]
const isMultiFace = computed(() => (card.value?.faces.length ?? 0) >= 2)
const hasSeparateFaceImages = computed(
  () => isMultiFace.value && SEPARATE_FACE_IMAGE_LAYOUTS.includes(card.value?.layout ?? ''),
)

// Rarity chip tint, echoing the familiar TCG rarity metals (uncommon silver, rare
// gold, mythic orange); anything unrecognised falls back to the muted chip.
const RARITY_CHIP_CLASSES: Record<string, string> = {
  common: 'bg-muted text-foreground/80',
  uncommon: 'bg-sky-500/15 text-sky-700 dark:text-sky-300',
  rare: 'bg-amber-500/15 text-amber-700 dark:text-amber-400',
  mythic: 'bg-orange-600/15 text-orange-700 dark:text-orange-400',
}
const rarityChipClass = computed(
  () => RARITY_CHIP_CLASSES[card.value?.rarity ?? ''] ?? 'bg-muted text-foreground/80',
)

// The finishes this card is actually priced in, so the price-alert dialog offers only those
// (a regular-only card shows no finish picker). Etched isn't surfaced in CardPrices — like the
// price summary and the toggleable chart, this is regular/foil only; a fully unpriced card
// falls back to regular so an alert can still be armed for when a price arrives.
const alertFinishes = computed<AlertFinish[]>(() => {
  const prices = card.value?.prices
  const finishes: AlertFinish[] = []
  if (prices?.usd != null) finishes.push('nonfoil')
  if (prices?.usd_foil != null) finishes.push('foil')
  return finishes.length ? finishes : ['nonfoil']
})
</script>

<template>
  <p v-if="notFound" class="text-destructive py-12">Card not found.</p>

  <template v-else>
    <!-- Header: name + mana cost, type line, and the at-a-glance chips (set, number,
      rarity). On a cache-miss deep link a Skeleton stands in until the query resolves;
      the chart, prints and sealed-product sections below mount off the route params
      immediately, so they fetch in parallel rather than waiting. -->
    <header v-if="card">
      <div class="flex flex-wrap items-start justify-between gap-x-6 gap-y-2">
        <div class="min-w-0">
          <h1 class="text-3xl font-semibold tracking-tight text-balance">{{ card.name }}</h1>
          <p v-if="card.type_line" class="text-muted-foreground mt-1">{{ card.type_line }}</p>
        </div>
        <p
          v-if="card.mana_cost && !isMultiFace"
          class="bg-muted/50 shrink-0 rounded-lg border px-3 py-1.5 text-lg"
          title="Mana cost"
        >
          <ManaSymbols :text="card.mana_cost" />
        </p>
      </div>
      <div class="mt-3 flex flex-wrap items-center gap-1.5 text-xs font-medium">
        <RouterLink
          :to="`/cards/${game}/sets/${card.set_code}`"
          class="bg-muted/50 hover:bg-muted inline-flex items-center gap-1.5 rounded-md border px-2 py-1 transition-colors"
        >
          {{ card.set_name }}
          <span class="text-muted-foreground">{{ card.set_code.toUpperCase() }}</span>
        </RouterLink>
        <span class="bg-muted/50 inline-flex items-center rounded-md border px-2 py-1 tabular-nums">
          #{{ card.collector_number }}
        </span>
        <span
          v-if="card.rarity"
          class="inline-flex items-center rounded-md px-2 py-1 capitalize"
          :class="rarityChipClass"
        >
          {{ card.rarity }}
        </span>
        <span v-if="releaseLabel" class="text-muted-foreground px-1">
          {{ releaseLabel.label }}
        </span>
      </div>
    </header>
    <div v-else class="space-y-3">
      <Skeleton class="h-9 w-2/3" />
      <Skeleton class="h-5 w-1/2" />
      <Skeleton class="h-6 w-80" />
    </div>

    <!-- Rows pinned to [auto,1fr]: row 1 hugs the rail's content and row 2 (the buy links)
      absorbs the spanning main column's surplus height — auto rows would instead split
      that surplus into row 1, opening a gap between the rail and the buy links. -->
    <div
      class="mt-8 grid items-start gap-8 md:grid-cols-[minmax(0,17rem)_1fr] md:grid-rows-[auto_1fr] md:gap-y-4 lg:grid-cols-[minmax(0,20rem)_1fr]"
    >
      <!-- Left rail: the image plus everything price/ownership shaped. -->
      <aside class="space-y-4 md:col-start-1 md:row-start-1">
        <template v-if="card">
          <!-- Image(s): one per face only for layouts with separate face images.
            Each is clickable to enlarge it in a lightbox (issue #53).
            Below md the rail is a full-width stack, so an uncapped image grows with the
            viewport — on the ~640-768px band (an unfolded foldable, a small tablet) that
            meant a card taller than the screen before any of its content (issue #573).
            Cap each image at the rail's own width from sm up and centre the row; phones
            keep the full-bleed image, and md+ hands sizing back to the rail column. -->
          <div
            class="flex justify-center gap-4 md:justify-start"
            :class="hasSeparateFaceImages ? 'flex-row md:flex-col' : ''"
          >
            <template v-if="hasSeparateFaceImages">
              <CardImageZoom
                v-for="(face, index) in card.faces"
                :key="index"
                :game="game"
                :id="card.id"
                :name="face.name ?? card.name"
                :face="index"
                class="w-full sm:max-w-72 md:max-w-none"
              />
            </template>
            <CardImageZoom
              v-else
              :game="game"
              :id="card.id"
              :name="card.name"
              :has-image="card.has_image"
              class="w-full sm:max-w-72 md:max-w-none"
            />
          </div>

          <!-- Prices -->
          <CardPriceSummary :card="card" />

          <!-- Watch this card's price (issue #525). In the shared body so it shows on both the
               full page and the browse-grid modal; shown to everyone (the dialog nudges
               signed-out visitors to make an account). -->
          <SetPriceAlertButton
            :game="game"
            target-kind="card"
            :external-id="card.id"
            :name="card.name"
            :finishes="alertFinishes"
          />

          <!-- Track how many copies you own (signed-in users). -->
          <CollectionControls :game="game" :card="card" />

          <!-- …and how many you want to buy — the wish-list twin (issue #167). Both
               gate internally on auth, so signed-out visitors see the nudges. -->
          <CollectionControls :game="game" :card="card" list="wishlist" />
        </template>
        <template v-else>
          <!-- Same sm cap as the real image, so the loading state doesn't reflow. -->
          <Skeleton
            class="mx-auto aspect-[61/85] w-full rounded-[4.76%_/_3.42%] sm:max-w-72 md:mx-0 md:max-w-none"
          />
          <Skeleton class="h-24 w-full" />
          <Skeleton class="h-28 w-full" />
        </template>
      </aside>

      <!-- Main column: rules text, details, price history, printings, sealed products.
        Spans both rail rows on md+, so the buy links slot under the rail beside it. -->
      <div class="min-w-0 space-y-6 md:col-start-2 md:row-span-2 md:row-start-1">
        <template v-if="card">
          <!-- Oracle text (single-faced cards; multi-faced show text per face below). -->
          <div
            v-if="!isMultiFace && card.oracle_text"
            class="bg-card rounded-xl border p-4 shadow-sm"
          >
            <p class="text-sm leading-relaxed whitespace-pre-line">
              <ManaSymbols :text="card.oracle_text" />
            </p>
          </div>

          <!-- Per-face text breakdown for any multi-faced card (incl. split/flip). -->
          <div v-if="isMultiFace" class="grid gap-3 sm:grid-cols-2">
            <div
              v-for="(face, index) in card.faces"
              :key="index"
              class="bg-card rounded-xl border p-4 shadow-sm"
            >
              <div class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
                <p class="font-medium">{{ face.name }}</p>
                <p v-if="face.mana_cost" class="text-muted-foreground text-sm">
                  <ManaSymbols :text="face.mana_cost" />
                </p>
              </div>
              <p v-if="face.type_line" class="text-muted-foreground text-sm">
                {{ face.type_line }}
              </p>
              <p v-if="face.oracle_text" class="mt-2 text-sm leading-relaxed whitespace-pre-line">
                <ManaSymbols :text="face.oracle_text" />
              </p>
              <p
                v-if="face.power && face.toughness"
                class="text-muted-foreground mt-2 text-sm tabular-nums"
              >
                {{ face.power }} / {{ face.toughness }}
              </p>
              <p v-if="face.loyalty" class="text-muted-foreground mt-2 text-sm tabular-nums">
                Loyalty {{ face.loyalty }}
              </p>
            </div>
          </div>

          <!-- The full details list — everything the chips summarise and more. -->
          <div class="bg-card rounded-xl border p-4 shadow-sm">
            <h2 class="mb-3 text-sm font-semibold">Details</h2>
            <CardMetaList :game="game" :card="card" />
          </div>

          <!-- Per-format legality, Scryfall-style (issue #557). Rides the card payload
            (no extra fetch); renders nothing for a card with no legality data. -->
          <CardLegalities :card="card" />
        </template>
        <template v-else>
          <Skeleton class="h-24 w-full rounded-xl" />
          <Skeleton class="h-40 w-full rounded-xl" />
        </template>

        <!-- Price history over time. Keyed off game/id, so it mounts and fetches in
          parallel with the card query above. `toggleable` adds the regular/foil key so
          either line can be switched off; `game` overlays set-release markers. -->
        <PriceChart
          :query-key="['card-prices', game, id]"
          :fetcher="(range) => getPriceHistory(game, id, range)"
          :game="game"
          toggleable
        />

        <!-- This card's other printings (same gameplay object, issue #63). Keyed off the
          route id so it mounts before the card loads; renders nothing when there are none. -->
        <CardPrints :game="game" :id="id" />

        <!-- Which sealed products this card is found in / can be pulled from / may be in.
          Renders nothing when the card is in no ingested product. -->
        <CardSealedProducts :game="game" :id="id" />

        <!-- The card's "Notes and Rules Information" (rulings, issue #522), last on the page.
          Keyed off the route id so it mounts before the card loads; renders nothing when
          there are none. -->
        <CardRulings :game="game" :id="id" />
      </div>

      <!-- Outbound "buy this card" links, grouped by region (issue #175). The rail's second
        row on md+ (right under the price/ownership stack) but LAST in source order, so the
        long store list doesn't push the card's actual content down on mobile. -->
      <CardBuyLinks v-if="card" class="md:col-start-1 md:row-start-2" :game="game" :card="card" />
    </div>
  </template>
</template>
