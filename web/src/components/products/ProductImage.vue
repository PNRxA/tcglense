<script setup lang="ts">
import { productImageUrl, type ProductImageSize } from '@/lib/api'
import { Package } from '@lucide/vue'
import { useImageLoad } from '@/composables/useImageLoad'

// Lazily-loaded sealed-product image via the caching proxy, the product-shaped
// sibling of CardImage. Sealed products (boxes, bundles, decks) have no fixed aspect
// ratio, so the frame is square with `object-contain` — the whole product shows,
// letterboxed against the page rather than cropped. A missing/failed image falls
// back to a muted placeholder keeping the frame shape.
const props = withDefaults(
  defineProps<{
    game: string
    id: string
    name: string
    size?: ProductImageSize
    hasImage?: boolean
  }>(),
  { size: 'normal', hasImage: true },
)

const { el, loaded, failed, onLoad, onError } = useImageLoad(() => [props.id, props.size])
</script>

<template>
  <div class="bg-muted/30 relative aspect-square overflow-hidden rounded-lg border">
    <template v-if="hasImage && !failed">
      <img
        ref="el"
        :src="productImageUrl(game, id, size)"
        :alt="name"
        loading="lazy"
        class="h-full w-full object-contain transition-opacity duration-500 ease-out motion-reduce:transition-none"
        :class="loaded ? 'opacity-100' : 'opacity-0'"
        @load="onLoad"
        @error="onError"
      />
      <div
        v-if="!loaded"
        class="bg-muted absolute inset-0 animate-pulse motion-reduce:animate-none"
        aria-hidden="true"
      />
    </template>
    <div
      v-else
      class="bg-muted text-muted-foreground absolute inset-0 flex flex-col items-center justify-center gap-2 text-center"
      :class="size === 'small' ? 'p-1' : 'p-3'"
    >
      <Package class="opacity-50" :class="size === 'small' ? 'size-5' : 'size-6'" />
      <!-- The roomy `normal` frame captions itself with the name; the compact `small`
        thumbnail (~40px dropdown / mover rows) can't fit it legibly and is always paired
        with a text label beside it, so it stays a bare glyph. -->
      <span v-if="size !== 'small'" class="text-xs leading-tight">{{ name }}</span>
    </div>
  </div>
</template>
