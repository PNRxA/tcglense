<script setup lang="ts">
import { computed, toRef } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { ArrowLeft } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import CardImageZoom from '@/components/cards/CardImageZoom.vue'
import CardMetaList from '@/components/cards/CardMetaList.vue'
import CardPriceSummary from '@/components/cards/CardPriceSummary.vue'
import CollectionControls from '@/components/cards/CollectionControls.vue'
import CardPrints from '@/components/cards/CardPrints.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import PriceChart from '@/components/cards/PriceChart.vue'
import { useCardBackLink } from '@/composables/useCardBackLink'
import { cardImageUrl, getCard } from '@/lib/api'
import { absoluteUrl, usePageMeta } from '@/lib/seo'

const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

const cardQuery = useQuery({
  queryKey: ['card', game, id],
  queryFn: () => getCard(game.value, id.value),
})

const card = computed(() => cardQuery.data.value)

// Absolute URL of the card's image (via the caching proxy) for the social preview
// and JSON-LD; undefined when the card has no image.
const cardImage = computed(() =>
  card.value?.has_image ? absoluteUrl(cardImageUrl(game.value, card.value.id, 'large')) : undefined,
)

const metaDescription = computed(() => {
  const c = card.value
  if (!c) return undefined
  const bits = [c.type_line, `${c.set_name} · #${c.collector_number}`].filter(
    (bit): bit is string => Boolean(bit),
  )
  return `${c.name} — ${bits.join(' · ')}. Prices and price history on TCGLense.`
})

// Product structured data (schema.org) so search engines can identify the card as
// a collectible product. We deliberately DON'T emit an `offers`/`InStock` block:
// this is a price-tracking page, not a storefront, so claiming the card is
// purchasable here would be a false structured-data assertion (and risks Google
// suppressing the rich result). Prices are shown to users but not marked up as an
// offer to sell.
const jsonLd = computed<Record<string, unknown> | undefined>(() => {
  const c = card.value
  if (!c) return undefined
  const data: Record<string, unknown> = {
    '@context': 'https://schema.org',
    '@type': 'Product',
    name: c.name,
    brand: { '@type': 'Brand', name: c.set_name },
  }
  if (cardImage.value) data.image = cardImage.value
  if (c.type_line) data.category = c.type_line
  return data
})

usePageMeta({
  title: () => card.value?.name,
  description: metaDescription,
  canonicalPath: () => (card.value ? `/cards/${game.value}/cards/${card.value.id}` : undefined),
  image: cardImage,
  type: 'product',
  jsonLd,
})

// The in-app "back" link, mirroring the path the user arrived by (issue #18/#63).
const backLink = useCardBackLink(game, card)

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
  <div class="mx-auto max-w-5xl px-4 py-10">
    <LoadingRow v-if="cardQuery.isPending.value" label="Loading card…" />
    <p v-else-if="cardQuery.isError.value || !card" class="text-destructive py-12">
      Card not found.
    </p>

    <template v-else-if="card">
      <RouterLink
        :to="backLink.to"
        class="text-muted-foreground hover:text-foreground mb-6 inline-flex items-center gap-1.5 text-sm"
      >
        <ArrowLeft class="size-4" />
        {{ backLink.label }}
      </RouterLink>

      <div class="grid gap-8 md:grid-cols-[minmax(0,20rem)_1fr]">
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
            {{ card.oracle_text }}
          </p>

          <!-- Per-face text breakdown for any multi-faced card (incl. split/flip). -->
          <div v-if="isMultiFace" class="mt-6 space-y-3">
            <div v-for="(face, index) in card.faces" :key="index" class="rounded-lg border p-3">
              <p class="font-medium">{{ face.name }}</p>
              <p v-if="face.type_line" class="text-muted-foreground text-sm">
                {{ face.type_line }}
              </p>
              <p v-if="face.mana_cost" class="text-muted-foreground text-sm">
                {{ face.mana_cost }}
              </p>
              <p v-if="face.oracle_text" class="mt-1 text-sm leading-relaxed whitespace-pre-line">
                {{ face.oracle_text }}
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
        </div>
      </div>

      <!-- Price history over time (full width, below the card details). -->
      <PriceChart :game="game" :id="card.id" />

      <!-- This card's other printings (same gameplay object, issue #63). Renders
        nothing when there are none. -->
      <CardPrints :game="game" :id="card.id" />
    </template>
  </div>
</template>
