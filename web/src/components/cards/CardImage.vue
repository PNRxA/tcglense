<script setup lang="ts">
import { ref, watch } from 'vue'
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

// Reset the error state if we point at a different card/face.
const failed = ref(false)
watch(
  () => [props.id, props.face, props.size],
  () => {
    failed.value = false
  },
)
</script>

<template>
  <div class="bg-muted relative aspect-[5/7] overflow-hidden rounded-xl">
    <img
      v-if="hasImage && !failed"
      :src="cardImageUrl(game, id, size, face)"
      :alt="name"
      loading="lazy"
      class="h-full w-full object-cover"
      @error="failed = true"
    />
    <div
      v-else
      class="text-muted-foreground absolute inset-0 flex flex-col items-center justify-center gap-2 p-3 text-center"
    >
      <ImageOff class="size-6 opacity-50" />
      <span class="text-xs leading-tight">{{ name }}</span>
    </div>
  </div>
</template>
