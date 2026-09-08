<script setup lang="ts">
import { computed } from 'vue'
import { Boxes, Layers, Package, Sparkles } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { setIconUrl } from '@/lib/api'
import type { PreconDeck, Product } from '@/lib/api'
import { useImageLoad } from '@/composables/useImageLoad'
import { productTypeLabel } from '@/lib/productType'
import { formatReleaseLabel } from '@/lib/releaseDate'
import {
  entryKindLabel,
  entryName,
  entryPath,
  setTypeLabel,
  type CalendarEntry,
} from '@/lib/releases'

// One row of the release calendar: a set (with the decks and sealed products it ships) or a
// Secret Lair drop (with the products that are it). A fact card — the name links to the
// entry's own page, the date reads "Releases …" / "Released …" by tense, and what ships is a
// list of links to pages that already exist; nothing here words a preview or a spoiler,
// because the catalog holds none.
const props = defineProps<{ game: string; entry: CalendarEntry }>()

const name = computed(() => entryName(props.entry))
const to = computed(() => entryPath(props.game, props.entry))
const kindLabel = computed(() => entryKindLabel(props.entry))
const set = computed(() => (props.entry.kind === 'set' ? props.entry.release.set : null))
const released = computed(() => formatReleaseLabel(props.entry.date, 'short'))

// The day-of-month block on the left — the calendar's one bit of visual grammar, read off the
// same local-midnight parse the label uses so the two never disagree on the day.
const dayParts = computed(() => {
  const [y, m, d] = props.entry.date.split('-').map(Number)
  const date = new Date(y ?? 0, (m ?? 1) - 1, d ?? 1)
  return {
    day: date.toLocaleDateString(undefined, { day: 'numeric' }),
    weekday: date.toLocaleDateString(undefined, { weekday: 'short' }),
  }
})

// The set's meta line: code, type, card count — the pieces a set tile shows.
const meta = computed(() => {
  if (!set.value) return null
  const parts = [set.value.code.toUpperCase()]
  const type = setTypeLabel(set.value.set_type)
  if (type) parts.push(type)
  if (set.value.card_count) parts.push(`${set.value.card_count.toLocaleString()} cards`)
  return parts.join(' · ')
})

const precons = computed<PreconDeck[]>(() =>
  props.entry.kind === 'set' ? props.entry.release.precons : [],
)
const products = computed<Product[]>(() => props.entry.release.products)

// A big set ships dozens of products; the row shows a handful and hands the rest to a browse
// that lists ALL of them, so the calendar stays a calendar. The cut is only made when such a
// browse exists — every row the API nested must stay reachable from the entry. The precon
// browse spans the set's whole catalog group (`?related=1`), matching how the API nested
// them; the sealed browse is per exact set code, so it stands in for the remainder only when
// every nested product is filed under the entry's own code (a child set's products would
// otherwise vanish behind "+N more"). A drop has no listing of its own at all, so it shows
// everything.
const SHOWN = 6
const morePreconsTo = computed(() =>
  set.value ? `/decks/${props.game}/precons/sets/${set.value.code}?related=1` : null,
)
const moreProductsTo = computed(() =>
  set.value && products.value.every((product) => product.set_code === set.value?.code)
    ? `/sealed/${props.game}/sets/${set.value.code}`
    : null,
)
const shownPrecons = computed(() =>
  morePreconsTo.value ? precons.value.slice(0, SHOWN) : precons.value,
)
const shownProducts = computed(() =>
  moreProductsTo.value ? products.value.slice(0, SHOWN) : products.value,
)

// The set icon through the caching proxy, with a graceful fallback; a drop shows the Secret
// Lair mark instead. Keyed on the set so a reused row reloads for its new set.
const {
  el: iconEl,
  loaded: iconLoaded,
  failed: iconFailed,
  onLoad,
  onError,
} = useImageLoad(() => [props.game, set.value?.code])
const showIcon = computed(() => !!set.value?.icon_svg_uri && !iconFailed.value)
</script>

<template>
  <article class="bg-card flex gap-4 rounded-xl border p-4">
    <div class="w-11 shrink-0 text-center" :title="entry.date">
      <p class="text-2xl leading-none font-semibold tabular-nums">{{ dayParts.day }}</p>
      <p class="text-muted-foreground mt-1 text-[0.625rem] tracking-wide uppercase">
        {{ dayParts.weekday }}
      </p>
    </div>

    <div class="flex size-10 shrink-0 items-center justify-center">
      <img
        v-if="set && showIcon"
        ref="iconEl"
        :src="setIconUrl(game, set.code)"
        alt=""
        class="size-8 object-contain transition-opacity duration-500 ease-out motion-reduce:transition-none dark:invert"
        :class="iconLoaded ? 'opacity-100' : 'opacity-0'"
        loading="lazy"
        @load="onLoad"
        @error="onError"
      />
      <Sparkles v-else-if="entry.kind === 'drop'" class="text-muted-foreground size-6" />
      <Layers v-else class="text-muted-foreground size-6" />
    </div>

    <div class="min-w-0 flex-1">
      <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
        <RouterLink :to="to" class="font-medium hover:underline">{{ name }}</RouterLink>
        <span class="bg-muted text-muted-foreground rounded-md px-1.5 py-0.5 text-xs">
          {{ kindLabel }}
        </span>
        <span
          v-if="released"
          class="rounded-md px-1.5 py-0.5 text-xs"
          :class="released.upcoming ? 'bg-info/15 text-info' : 'bg-muted text-muted-foreground'"
        >
          {{ released.label }}
        </span>
      </div>
      <p v-if="meta" class="text-muted-foreground mt-0.5 text-xs">{{ meta }}</p>

      <div v-if="precons.length" class="mt-3">
        <p class="text-muted-foreground flex items-center gap-1.5 text-xs font-medium">
          <Boxes class="size-3.5" />
          {{ precons.length === 1 ? 'Preconstructed deck' : 'Preconstructed decks' }}
        </p>
        <ul class="mt-1.5 flex flex-wrap gap-1.5">
          <li v-for="precon in shownPrecons" :key="precon.slug">
            <RouterLink
              :to="`/decks/${game}/precons/${precon.slug}`"
              class="hover:bg-accent inline-flex items-center gap-1 rounded-md border px-2 py-1 text-sm transition-colors"
            >
              <span class="truncate">{{ precon.name }}</span>
              <span class="text-muted-foreground text-xs">· {{ precon.deck_type }}</span>
            </RouterLink>
          </li>
          <li v-if="precons.length > shownPrecons.length && morePreconsTo">
            <RouterLink
              :to="morePreconsTo"
              class="text-muted-foreground inline-flex items-center rounded-md border border-dashed px-2 py-1 text-sm hover:underline"
            >
              +{{ precons.length - shownPrecons.length }} more
            </RouterLink>
          </li>
        </ul>
      </div>

      <div v-if="products.length" class="mt-3">
        <p class="text-muted-foreground flex items-center gap-1.5 text-xs font-medium">
          <Package class="size-3.5" />
          {{ products.length === 1 ? 'Sealed product' : 'Sealed products' }}
        </p>
        <ul class="mt-1.5 flex flex-wrap gap-1.5">
          <li v-for="product in shownProducts" :key="product.id">
            <RouterLink
              :to="`/sealed/${game}/${product.id}`"
              class="hover:bg-accent inline-flex items-center gap-1 rounded-md border px-2 py-1 text-sm transition-colors"
            >
              <span class="truncate">{{ product.name }}</span>
              <span class="text-muted-foreground text-xs">
                · {{ productTypeLabel(product.product_type) }}
              </span>
            </RouterLink>
          </li>
          <li v-if="products.length > shownProducts.length && moreProductsTo">
            <RouterLink
              :to="moreProductsTo"
              class="text-muted-foreground inline-flex items-center rounded-md border border-dashed px-2 py-1 text-sm hover:underline"
            >
              +{{ products.length - shownProducts.length }} more
            </RouterLink>
          </li>
        </ul>
      </div>
    </div>
  </article>
</template>
