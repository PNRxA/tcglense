<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { ref, watch } from 'vue'
import { X, ZoomIn } from '@lucide/vue'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import CardImage from '@/components/cards/CardImage.vue'
import { cn } from '@/lib/utils'
import type { ImageSize } from '@/lib/api'

// A clickable wrapper around CardImage that opens the image in a modal lightbox
// at the highest available resolution (issue #53). Used on the card detail page.
// The ui/dialog primitives give us a focus trap, Escape-to-close, scroll lock,
// and click-outside dismissal for free.
const props = withDefaults(
  defineProps<{
    game: string
    id: string
    name: string
    face?: number
    hasImage?: boolean
    /** Size shown inline on the page (the thumbnail). */
    size?: ImageSize
    /** Size shown enlarged in the lightbox — highest-res by default. */
    zoomSize?: ImageSize
    class?: HTMLAttributes['class']
  }>(),
  { size: 'large', zoomSize: 'png', hasImage: true },
)

// Two possible roots (placeholder vs. dialog); apply `class` explicitly so it
// always lands on the rendered element rather than via attribute fallthrough.
defineOptions({ inheritAttrs: false })

// Track a runtime load failure so a card that *claims* an image but fails to
// fetch it degrades to the plain placeholder instead of offering a zoom that
// would only enlarge that placeholder. Reset when we point at a different image.
const failed = ref(false)
watch(
  () => [props.id, props.face, props.size],
  () => {
    failed.value = false
  },
)
</script>

<template>
  <!-- Nothing to enlarge (no image, or it failed to load): render the plain
    placeholder, not a zoom affordance. -->
  <CardImage
    v-if="!hasImage || failed"
    :game="game"
    :id="id"
    :name="name"
    :face="face"
    :has-image="false"
    :size="size"
    :class="props.class"
  />

  <Dialog v-else>
    <!-- The trigger button overlays the image rather than wrapping it, so the
      <button> holds only phrasing content (the affordance) while still covering
      the whole image as the click target. -->
    <div :class="cn('relative', props.class)">
      <CardImage
        :game="game"
        :id="id"
        :name="name"
        :face="face"
        :size="size"
        @error="failed = true"
      />
      <DialogTrigger
        class="group focus-visible:border-ring focus-visible:ring-ring/50 absolute inset-0 cursor-zoom-in rounded-xl outline-none focus-visible:ring-3"
        :aria-label="`Enlarge image of ${name}`"
      >
        <!-- Hover/focus affordance: a zoom glyph hinting the image opens larger. -->
        <span
          class="absolute inset-0 flex items-center justify-center opacity-0 transition-opacity duration-200 group-hover:opacity-100 group-focus-visible:opacity-100 motion-reduce:transition-none"
          aria-hidden="true"
        >
          <span
            class="bg-background/70 flex size-12 items-center justify-center rounded-full shadow-md"
          >
            <ZoomIn class="size-6" />
          </span>
        </span>
      </DialogTrigger>
    </div>

    <!-- Width is the lesser of 90vw and the width a 61:85 card image needs to stand
      90vh tall (61:85 = Scryfall's card-image ratio, matching CardImage's frame), so
      a standard card always fits the viewport; off-ratio art letterboxes inside via
      the image's own object-contain. -->
    <DialogContent class="w-[min(90vw,calc(90vh*61/85))]">
      <DialogTitle class="sr-only">{{ name }}</DialogTitle>
      <DialogDescription class="sr-only">
        Enlarged card image. Press Escape or click outside to close.
      </DialogDescription>

      <CardImage
        :game="game"
        :id="id"
        :name="name"
        :face="face"
        :size="zoomSize"
        class="w-full shadow-2xl"
      />

      <DialogClose
        class="bg-background hover:bg-accent focus-visible:border-ring focus-visible:ring-ring/50 absolute -top-3 -right-3 flex size-9 items-center justify-center rounded-full border shadow-lg transition-colors outline-none focus-visible:ring-3"
        aria-label="Close"
      >
        <X class="size-5" />
      </DialogClose>
    </DialogContent>
  </Dialog>
</template>
