<script setup lang="ts">
import { computed, ref } from 'vue'
import { Layers } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { setIconUrl, type CardSet } from '@/lib/api'

const props = defineProps<{
  game: string
  set: CardSet
}>()

// Show the icon (served through our caching proxy) when the set has one, with a
// graceful fallback if the fetch fails.
const iconFailed = ref(false)
const showIcon = computed(() => !!props.set.icon_svg_uri && !iconFailed.value)

const released = computed(() => {
  if (!props.set.released_at) return null
  const date = new Date(props.set.released_at)
  if (Number.isNaN(date.getTime())) return props.set.released_at
  return date.toLocaleDateString(undefined, { year: 'numeric', month: 'short' })
})
</script>

<template>
  <RouterLink
    :to="`/cards/${game}/sets/${set.code}`"
    class="bg-card hover:border-ring/60 hover:bg-accent/40 flex items-center gap-3 rounded-xl border p-3 transition-colors"
  >
    <div class="flex size-10 shrink-0 items-center justify-center">
      <img
        v-if="showIcon"
        :src="setIconUrl(game, set.code)"
        alt=""
        class="size-8 object-contain dark:invert"
        loading="lazy"
        @error="iconFailed = true"
      />
      <Layers v-else class="text-muted-foreground size-6" />
    </div>
    <div class="min-w-0">
      <p class="truncate font-medium" :title="set.name">{{ set.name }}</p>
      <p class="text-muted-foreground truncate text-xs">
        {{ set.code.toUpperCase() }}
        <template v-if="released"> · {{ released }}</template>
        <template v-if="set.card_count"> · {{ set.card_count }} cards</template>
      </p>
    </div>
  </RouterLink>
</template>
