<script setup lang="ts">
import { computed } from 'vue'
import { Package } from '@lucide/vue'
import { RouterLink, useRouter } from 'vue-router'
import { setIconUrl, type CardSet } from '@/lib/api'
import { formatCopies } from '@/lib/ownership'
import { useImageLoad } from '@/composables/useImageLoad'
import { prefetchRouteChunks } from '@/lib/prefetch'

// A standalone bordered tile for one product set — the sealed-product mirror of the cards
// landing's SetTile (default variant). Shared by the two sealed set-tile landings: the public
// catalog landing (SealedGameView, a plain product count) and the collection/wish-list holdings
// section (ProductHoldingSection, which also passes the held copies + total value). It clicks
// through to a set-scoped products list rather than rendering the products inline. The catalog
// set (icon + release date) is optional: a set with no catalog row falls back to a Package icon
// and drops the release date. No completion count (sealed products aren't a fixed-size set) and
// no height-reserving spacer (these tiles never nest sub-sets, so rows can't get out of step).
const props = defineProps<{
  game: string
  // Set identity — the tile shows `name`, falling back to the upper-cased `code`.
  code: string
  name: string | null
  // The distinct-product count shown on the stats line (the catalog set's product count, or the
  // holdings section's unique held products).
  products: number
  // The matching catalog set — the icon and release-date source. Undefined when the set's code
  // has no catalog row.
  catalogSet?: CardSet
  // Where the tile links (the set-scoped products list).
  to: string
  // Total copies including duplicates (the holdings section's `total_products`). Shown as
  // "N copies" only when it exceeds `products`; omitted by the catalog landing, where each set
  // has a single count.
  copies?: number
  // Preformatted total value (e.g. "$123.45"), shown as "TOTAL $X" on the identity line. The
  // parent formats it via useCurrency, matching SetTile's `ownedValue`. Null hides the stat.
  value?: string | null
}>()

const displayName = computed(() => props.name ?? props.code.toUpperCase())

// Warm the destination's JS chunk on hover/focus so the click opens a loaded view
// (see lib/prefetch.ts — chunks only, never data/images).
const router = useRouter()
const warm = () => prefetchRouteChunks(router, props.to)

// Show the set icon (served through our caching proxy) when the catalog set has one, with a
// graceful fallback if the fetch fails. The load state resets when we point at a different set.
const {
  el: iconEl,
  loaded: iconLoaded,
  failed: iconFailed,
  onLoad,
  onError,
} = useImageLoad(() => [props.game, props.code])
const showIcon = computed(() => !!props.catalogSet?.icon_svg_uri && !iconFailed.value)

// The catalog set's release month/year, formatted exactly like SetTile's — null when there's
// no catalog row or no release date.
const released = computed(() => {
  const releasedAt = props.catalogSet?.released_at
  if (!releasedAt) return null
  const date = new Date(releasedAt)
  if (Number.isNaN(date.getTime())) return releasedAt
  return date.toLocaleDateString(undefined, { year: 'numeric', month: 'short' })
})

// The total copies (with duplicates) as "N copies", shown next to the product count only when
// there are more copies than distinct products — otherwise it just restates the count (SetTile's
// copies rule). The catalog landing omits `copies` entirely, so it never shows.
const copiesLabel = computed(() =>
  props.copies != null && props.copies > props.products ? formatCopies(props.copies) : null,
)
</script>

<template>
  <RouterLink
    :to="to"
    class="bg-card hover:border-ring/60 hover:bg-accent/40 block rounded-xl border p-3 transition-colors"
    @pointerenter="warm"
    @focusin="warm"
  >
    <div class="flex items-center gap-3">
      <div class="flex size-10 shrink-0 items-center justify-center">
        <img
          v-if="showIcon"
          ref="iconEl"
          :src="setIconUrl(game, code)"
          alt=""
          class="size-8 object-contain transition-opacity duration-500 ease-out motion-reduce:transition-none dark:invert"
          :class="iconLoaded ? 'opacity-100' : 'opacity-0'"
          loading="lazy"
          @load="onLoad"
          @error="onError"
        />
        <Package v-else class="text-muted-foreground size-6" />
      </div>
      <div class="min-w-0 flex-1">
        <p class="truncate font-medium" :title="displayName">{{ displayName }}</p>
        <!-- Set identity (code + release), with the total value pinned to the right in
             SetTile's "TOTAL $X" idiom so the identity truncates first and the worth stays. -->
        <div class="text-muted-foreground flex items-baseline justify-between gap-2 text-xs">
          <span class="min-w-0 truncate">
            {{ code.toUpperCase() }}
            <template v-if="released"> · {{ released }}</template>
          </span>
          <span
            v-if="value"
            class="text-foreground shrink-0 font-medium tabular-nums"
            title="Total estimated value"
          >
            <span class="text-muted-foreground text-[0.625rem] tracking-wide uppercase">Total</span>
            {{ value }}
          </span>
        </div>
        <!-- The held-product stats: distinct products, plus total copies when there are
             duplicates. -->
        <div class="text-muted-foreground mt-0.5 truncate text-xs tabular-nums">
          {{ products.toLocaleString() }}
          {{ products === 1 ? 'product' : 'products'
          }}<template v-if="copiesLabel"> · {{ copiesLabel }}</template>
        </div>
      </div>
    </div>
  </RouterLink>
</template>
