<script setup lang="ts">
import { computed } from 'vue'
import type { DeckStatItem } from '@/lib/deckStats'

const props = defineProps<{ title: string; items: DeckStatItem[] }>()
const maximum = computed(() => Math.max(1, ...props.items.map((item) => item.count)))
</script>

<template>
  <section>
    <h3 class="mb-3 text-sm font-semibold">{{ title }}</h3>
    <div class="space-y-2">
      <div v-for="item in items" :key="item.key">
        <div class="mb-1 flex items-center justify-between gap-3 text-xs">
          <span>{{ item.label }}</span>
          <span class="text-muted-foreground tabular-nums">{{ item.count }}</span>
        </div>
        <div class="bg-muted h-2 overflow-hidden rounded-full">
          <div
            class="h-full min-w-px rounded-full transition-[width]"
            :style="{
              width: `${(item.count / maximum) * 100}%`,
              backgroundColor: item.color ?? 'var(--primary)',
            }"
            role="img"
            :aria-label="`${item.label}: ${item.count} copies`"
          />
        </div>
      </div>
    </div>
  </section>
</template>
