<script setup lang="ts">
import { computed, nextTick, onMounted, ref, useTemplateRef, watch } from 'vue'
import { Layers } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { setIconUrl, type CardSet } from '@/lib/api'

const props = withDefaults(
  defineProps<{
    game: string
    set: CardSet
    // 'default' — standalone bordered tile. 'plain' — borderless, used as the
    // header of a SetGroup card. 'nested' — compact row for a sub-set.
    variant?: 'default' | 'plain' | 'nested'
    // Optional display-name override (e.g. the parent prefix stripped for
    // nested rows). Falls back to the set's own name.
    label?: string
  }>(),
  { variant: 'default', label: undefined },
)

const nested = computed(() => props.variant === 'nested')
const displayName = computed(() => props.label ?? props.set.name)
// When the visible label is abbreviated (a stripped sub-set name), keep the full
// set name as the link's accessible name so it stays unambiguous out of its
// visual group (WCAG 2.4.4).
const linkLabel = computed(() =>
  props.label && props.label !== props.set.name ? props.set.name : undefined,
)

const rootClass = computed(() => {
  switch (props.variant) {
    case 'plain':
      return 'hover:bg-accent/40 rounded-t-xl p-3'
    case 'nested':
      return 'hover:bg-accent/50 rounded-lg p-2'
    default:
      return 'bg-card hover:border-ring/60 hover:bg-accent/40 rounded-xl border p-3'
  }
})

// Show the icon (served through our caching proxy) when the set has one, with a
// graceful fallback if the fetch fails.
const iconFailed = ref(false)
const iconLoaded = ref(false)
const iconEl = useTemplateRef<HTMLImageElement>('iconEl')
const showIcon = computed(() => !!props.set.icon_svg_uri && !iconFailed.value)

// A cached icon can finish loading before the `load` listener is attached, so its
// event never fires. Reflect the already-complete state so it doesn't stay stuck
// at opacity-0 waiting for an event that won't come (mirrors CardImage).
function syncIconLoaded() {
  const el = iconEl.value
  if (el?.complete && el.naturalWidth > 0) iconLoaded.value = true
}

onMounted(syncIconLoaded)

// SetTile instances are reused across a set list (v-for), so reset + re-check when
// we point at a different set's icon (it may resolve instantly from cache).
watch(
  () => [props.game, props.set.code],
  () => {
    iconFailed.value = false
    iconLoaded.value = false
    nextTick(syncIconLoaded)
  },
)

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
    class="block transition-colors"
    :class="rootClass"
    :aria-label="linkLabel"
  >
    <div class="flex items-center gap-3">
      <div class="flex shrink-0 items-center justify-center" :class="nested ? 'size-8' : 'size-10'">
        <img
          v-if="showIcon"
          ref="iconEl"
          :src="setIconUrl(game, set.code)"
          alt=""
          class="object-contain transition-opacity duration-500 ease-out motion-reduce:transition-none dark:invert"
          :class="[nested ? 'size-6' : 'size-8', iconLoaded ? 'opacity-100' : 'opacity-0']"
          loading="lazy"
          @load="iconLoaded = true"
          @error="iconFailed = true"
        />
        <Layers v-else class="text-muted-foreground" :class="nested ? 'size-5' : 'size-6'" />
      </div>
      <div class="min-w-0">
        <p class="truncate font-medium" :class="nested ? 'text-sm' : ''" :title="set.name">
          {{ displayName }}
        </p>
        <p class="text-muted-foreground truncate text-xs">
          {{ set.code.toUpperCase() }}
          <template v-if="released"> · {{ released }}</template>
          <template v-if="set.card_count"> · {{ set.card_count }} cards</template>
        </p>
      </div>
    </div>
    <!-- A standalone tile (no collapsible sub-sets) reserves the height of the
         footer row that SetGroup renders below its header, so a childless tile
         lines up with the collapsible tiles sharing its row instead of looking
         stunted (2.25rem = the h-7 "View all" button that drives the row height
         + its pb-2). Only needed once the grid is multi-column (sm+); in the
         single-column layout there is no row neighbour to match, so the tile
         stays compact. -->
    <div v-if="variant === 'default'" aria-hidden="true" class="hidden h-9 sm:block"></div>
  </RouterLink>
</template>
