<script setup lang="ts">
import { computed } from 'vue'
import { Skeleton } from '@/components/ui/skeleton'
import { CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'

// Placeholder for CardGrid while a browse grid's first page loads. Reproduces the
// real grid's column mechanism — the same wrapper classes and the persisted card-size
// density (CARD_SIZE_GRID_CLASS) — so the incoming layout is reserved and back/forward
// scroll restore lands on a plausible height. Tiles copy CardImage's 61:85 frame and
// corner radius so they read as cards, not blank boxes.
withDefaults(defineProps<{ count?: number }>(), { count: 12 })

const cardSize = useCardSizeStore()
const gridClass = computed(() => CARD_SIZE_GRID_CLASS[cardSize.size])
</script>

<template>
  <div class="grid gap-x-4 gap-y-6" :class="gridClass">
    <Skeleton v-for="i in count" :key="i" class="aspect-[61/85] rounded-[4.76%_/_3.42%]" />
  </div>
</template>
