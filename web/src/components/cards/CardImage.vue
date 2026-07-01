<script setup lang="ts">
import { cardImageUrl, type ImageSize } from '@/lib/api'
import { ImageOff } from '@lucide/vue'
import { useImageLoad } from '@/composables/useImageLoad'

const props = withDefaults(
  defineProps<{
    game: string
    id: string
    name: string
    size?: ImageSize
    face?: number
    hasImage?: boolean
  }>(),
  { size: 'normal', hasImage: true },
)

// `error` lets a parent (e.g. CardImageZoom) react when an image that claimed to
// exist fails to load at runtime, so it can stop offering to enlarge a placeholder.
const emit = defineEmits<{ error: [] }>()

// Reset the load state whenever we point at a different card/face/size.
const { el, loaded, failed, onLoad, onError } = useImageLoad(() => [
  props.id,
  props.face,
  props.size,
])

function handleError() {
  onError()
  emit('error')
}
</script>

<template>
  <!-- Corners match a real MTG card: a 63×88 mm card has a ~3 mm corner radius, so
    the radius is 3/63 ≈ 4.76% of the width and 3/88 ≈ 3.4% of the height. Expressing
    both as percentages (resolved against this frame's own 5:7 border-box) keeps the
    radius proportional at every card size and, because 3.4% × 7/5 = 4.76%, keeps the
    corner a true circle rather than an ellipse. `shadow-sm` lifts the card off the
    page; CardTile deepens it (and scales the card up) on hover. In dark mode a black
    shadow is invisible against the near-black background, so we swap in a larger,
    higher-opacity shadow so the lift still reads. -->
  <div
    class="relative aspect-[5/7] overflow-hidden rounded-[4.76%_/_3.4%] shadow-sm dark:shadow-[0_2px_8px_rgba(0,0,0,0.6)]"
  >
    <template v-if="hasImage && !failed">
      <!-- `object-contain`, not `object-cover`: Scryfall renders most cards slightly
        wider than the 5:7 frame, so `cover` would slice off the card's left/right
        borders, and off-ratio printings (landscape plane/scheme/art-series cards)
        would be cropped hard. Contain shows the whole card. The frame has no fill,
        so the letterbox shows the page background rather than a muted rectangle
        peeking out around the image. -->
      <img
        ref="el"
        :src="cardImageUrl(game, id, size, face)"
        :alt="name"
        loading="lazy"
        class="h-full w-full object-contain transition-opacity duration-500 ease-out motion-reduce:transition-none"
        :class="loaded ? 'opacity-100' : 'opacity-0'"
        @load="onLoad"
        @error="handleError"
      />
      <!-- Pulsing skeleton fills the frame until the image bytes arrive and fade
        in; it's removed on load so it never shows behind an off-ratio card. -->
      <div
        v-if="!loaded"
        class="bg-muted absolute inset-0 animate-pulse motion-reduce:animate-none"
        aria-hidden="true"
      />
    </template>
    <!-- Failed / no-image: a deliberate placeholder, so it keeps the muted card
      shape rather than floating on the page background. -->
    <div
      v-else
      class="bg-muted text-muted-foreground absolute inset-0 flex flex-col items-center justify-center gap-2 p-3 text-center"
    >
      <ImageOff class="size-6 opacity-50" />
      <span class="text-xs leading-tight">{{ name }}</span>
    </div>
  </div>
</template>
