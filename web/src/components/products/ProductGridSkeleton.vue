<script setup lang="ts">
import { computed } from 'vue'
import { Skeleton } from '@/components/ui/skeleton'
import { CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'

// Placeholder for ProductGrid while the sealed grid's first page loads. Reproduces the
// real grid's column mechanism — the same wrapper classes and the persisted card-size
// density — so the incoming layout is reserved. Tiles copy ProductImage's square frame
// and rounding, with two text-line skeletons standing in for the name/meta lines.
withDefaults(defineProps<{ count?: number }>(), { count: 12 })

const cardSize = useCardSizeStore()
const gridClass = computed(() => CARD_SIZE_GRID_CLASS[cardSize.size])
</script>

<template>
  <div class="grid gap-x-4 gap-y-6" :class="gridClass">
    <div v-for="i in count" :key="i">
      <Skeleton class="aspect-square rounded-lg" />
      <Skeleton class="mt-1.5 h-4 w-full" />
      <Skeleton class="mt-1.5 h-3 w-2/3" />
    </div>
  </div>
</template>
