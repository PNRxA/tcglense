<script setup lang="ts">
import { onMounted, nextTick, ref, useTemplateRef, watch } from 'vue'
import { ImageOff } from '@lucide/vue'
import { cardImageUrl, type ImageSize } from '@/lib/api'

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

const failed = ref(false)
const loaded = ref(false)
const imgEl = useTemplateRef<HTMLImageElement>('imgEl')

// A cached image can finish loading before the `load` listener is attached, so
// its event never fires. Reflect the already-complete state so the card never
// stays stuck invisible (at opacity-0) waiting for an event that won't come.
function syncLoaded() {
  const el = imgEl.value
  if (el?.complete && el.naturalWidth > 0) loaded.value = true
}

onMounted(syncLoaded)

// Reset state when we point at a different card/face/size, then re-check once
// the new src is in the DOM (it may resolve instantly from cache).
watch(
  () => [props.id, props.face, props.size],
  () => {
    failed.value = false
    loaded.value = false
    nextTick(syncLoaded)
  },
)
</script>

<template>
  <div class="bg-muted relative aspect-[5/7] overflow-hidden rounded-xl">
    <template v-if="hasImage && !failed">
      <!-- `object-contain`, not `object-cover`: Scryfall renders cards slightly
        wider than the 5:7 frame, so `cover` would slice off the card's left/right
        borders. Contain shows the whole card; any sub-pixel letterbox blends into
        the bg-muted backdrop. -->
      <img
        ref="imgEl"
        :src="cardImageUrl(game, id, size, face)"
        :alt="name"
        loading="lazy"
        class="h-full w-full object-contain transition-opacity duration-500 ease-out motion-reduce:transition-none"
        :class="loaded ? 'opacity-100' : 'opacity-0'"
        @load="loaded = true"
        @error="failed = true"
      />
      <!-- Pulsing skeleton until the image bytes arrive and fade in. -->
      <div
        v-if="!loaded"
        class="bg-muted-foreground/10 absolute inset-0 animate-pulse motion-reduce:animate-none"
        aria-hidden="true"
      />
    </template>
    <div
      v-else
      class="text-muted-foreground absolute inset-0 flex flex-col items-center justify-center gap-2 p-3 text-center"
    >
      <ImageOff class="size-6 opacity-50" />
      <span class="text-xs leading-tight">{{ name }}</span>
    </div>
  </div>
</template>
