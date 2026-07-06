<script setup lang="ts">
import { computed, toRef } from 'vue'
import CardImageZoom from '@/components/cards/CardImageZoom.vue'
import CardMetaList from '@/components/cards/CardMetaList.vue'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'
import CardPriceSummary from '@/components/cards/CardPriceSummary.vue'
import CollectionControls from '@/components/collection/CollectionControls.vue'
import CardPrints from '@/components/cards/CardPrints.vue'
import CardSealedProducts from '@/components/products/CardSealedProducts.vue'
import CardBuyLinks from '@/components/cards/CardBuyLinks.vue'
import PriceChart from '@/components/cards/PriceChart.vue'
import { Skeleton } from '@/components/ui/skeleton'
import { useCardQuery } from '@/composables/useCatalog'
import { getPriceHistory } from '@/lib/api'

// The body of a card's detail — image(s), rules text, prices + history, collection and
// wish-list count controls, other printings — shared verbatim by the full page
// (CardDetailView) and the browse-grid modal (CardDetailDialog). Page chrome (meta
// tags, breadcrumb/back link, the modal frame) stays with the callers; both fetch the
// same ['card', game, id] key, so the page and an overlay never double-fetch.
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

const cardQuery = useCardQuery(game, id)

const card = computed(() => cardQuery.data.value)
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
</script>

<template>
  <p v-if="notFound" class="text-destructive py-12">Card not found.</p>

  <template v-else>
    <!-- Card body — image(s) + details. On a cache-miss deep link a Skeleton stands in
      until the query resolves; the chart, prints and sealed-product sections below mount
      off the route params immediately, so they fetch in parallel rather than waiting. -->
    <div v-if="!card" class="grid gap-8 md:grid-cols-[minmax(0,20rem)_1fr]">
      <Skeleton class="aspect-[61/85] w-full rounded-[4.76%_/_3.42%]" />
      <div class="space-y-4">
        <Skeleton class="h-9 w-2/3" />
        <Skeleton class="h-5 w-1/2" />
        <Skeleton class="h-24 w-full" />
        <Skeleton class="h-28 w-full" />
      </div>
    </div>

    <div v-else class="grid gap-8 md:grid-cols-[minmax(0,20rem)_1fr]">
      <!-- Image(s): one per face only for layouts with separate face images.
        Each is clickable to enlarge it in a lightbox (issue #53). -->
      <div class="flex gap-4" :class="hasSeparateFaceImages ? 'flex-row md:flex-col' : ''">
        <template v-if="hasSeparateFaceImages">
          <CardImageZoom
            v-for="(face, index) in card.faces"
            :key="index"
            :game="game"
            :id="card.id"
            :name="face.name ?? card.name"
            :face="index"
            class="w-full"
          />
        </template>
        <CardImageZoom
          v-else
          :game="game"
          :id="card.id"
          :name="card.name"
          :has-image="card.has_image"
          class="w-full"
        />
      </div>

      <!-- Details -->
      <div>
        <h1 class="text-3xl font-semibold tracking-tight">{{ card.name }}</h1>
        <p v-if="card.type_line" class="text-muted-foreground mt-1">{{ card.type_line }}</p>

        <CardMetaList :game="game" :card="card" />

        <!-- Oracle text (single-faced cards; multi-faced show text per face below). -->
        <p
          v-if="!isMultiFace && card.oracle_text"
          class="mt-6 text-sm leading-relaxed whitespace-pre-line"
        >
          <ManaSymbols :text="card.oracle_text" />
        </p>

        <!-- Per-face text breakdown for any multi-faced card (incl. split/flip). -->
        <div v-if="isMultiFace" class="mt-6 space-y-3">
          <div v-for="(face, index) in card.faces" :key="index" class="rounded-lg border p-3">
            <p class="font-medium">{{ face.name }}</p>
            <p v-if="face.type_line" class="text-muted-foreground text-sm">
              {{ face.type_line }}
            </p>
            <p v-if="face.mana_cost" class="text-muted-foreground text-sm">
              <ManaSymbols :text="face.mana_cost" />
            </p>
            <p v-if="face.oracle_text" class="mt-1 text-sm leading-relaxed whitespace-pre-line">
              <ManaSymbols :text="face.oracle_text" />
            </p>
            <p
              v-if="face.power && face.toughness"
              class="text-muted-foreground mt-1 text-sm tabular-nums"
            >
              {{ face.power }} / {{ face.toughness }}
            </p>
            <p v-if="face.loyalty" class="text-muted-foreground mt-1 text-sm tabular-nums">
              Loyalty {{ face.loyalty }}
            </p>
          </div>
        </div>

        <!-- Prices -->
        <CardPriceSummary :card="card" />

        <!-- Track how many copies you own (signed-in users). -->
        <CollectionControls :game="game" :card="card" />

        <!-- …and how many you want to buy — the wish-list twin (issue #167). Both
             gate internally on auth, so signed-out visitors see the nudges. -->
        <CollectionControls :game="game" :card="card" list="wishlist" />
      </div>
    </div>

    <!-- Price history over time (full width, below the card details). -->
    <PriceChart
      :query-key="['card-prices', game, id]"
      :fetcher="(range) => getPriceHistory(game, id, range)"
    />

    <!-- Outbound "buy this card" links, grouped by region (issue #175). Needs the full
      card object, so it waits for the fetch (the sections below key off game/id alone). -->
    <CardBuyLinks v-if="card" :game="game" :card="card" />

    <!-- This card's other printings (same gameplay object, issue #63). Keyed off the
      route id so it mounts before the card loads; renders nothing when there are none. -->
    <CardPrints :game="game" :id="id" />

    <!-- Which sealed products this card is found in / can be pulled from / may be in.
      Renders nothing when the card is in no ingested product. -->
    <CardSealedProducts :game="game" :id="id" />
  </template>
</template>
