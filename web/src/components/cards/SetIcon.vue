<script setup lang="ts">
import { computed } from 'vue'
import { Layers } from '@lucide/vue'
import { setIconUrl } from '@/lib/api'
import { useImageLoad } from '@/composables/useImageLoad'

// A set's icon through the caching proxy, in SetTile's idiom (a fade-in on load, the
// generic Layers glyph when the set has no icon or the fetch fails), sized to sit where a
// card or product thumbnail would — the universal search's set rows. Decorative: the row
// beside it names the set, so the image carries no alt text.
const props = defineProps<{
  game: string
  code: string
  /** Whether the catalog has an icon for this set (`icon_svg_uri` present). */
  hasIcon: boolean
}>()

const {
  el: iconEl,
  loaded: iconLoaded,
  failed: iconFailed,
  onLoad,
  onError,
} = useImageLoad(() => [props.game, props.code])
const showIcon = computed(() => props.hasIcon && !iconFailed.value)
</script>

<template>
  <span
    class="bg-muted flex size-10 shrink-0 items-center justify-center rounded-md"
    aria-hidden="true"
  >
    <img
      v-if="showIcon"
      ref="iconEl"
      :src="setIconUrl(game, code)"
      alt=""
      class="size-6 object-contain transition-opacity duration-500 ease-out motion-reduce:transition-none dark:invert"
      :class="iconLoaded ? 'opacity-100' : 'opacity-0'"
      loading="lazy"
      @load="onLoad"
      @error="onError"
    />
    <Layers v-else class="text-muted-foreground size-4" />
  </span>
</template>
