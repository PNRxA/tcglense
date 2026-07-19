<script setup lang="ts">
import { computed } from 'vue'

// A flex-wrapping row of summary stats (a `<dl>` of label → value pairs) shared by the
// collection and wish-list landings: the top-of-page combined overview and the per-section
// (cards / sealed) breakdowns all render through here so every stat block formats
// identically. An item whose value is `null`/`undefined` is dropped, so a caller can pass an
// unpriced money value straight through and have just that stat self-hide (matching the old
// per-`<dl>` `v-if` gates); pass an empty `items` array and the whole list renders nothing.
// `size="lg"` bumps the value type scale for the combined overview so it reads as the
// headline above the smaller per-section rows.
export interface StatItem {
  label: string
  value: string | null | undefined
}

const props = withDefaults(
  defineProps<{
    items: StatItem[]
    size?: 'md' | 'lg'
  }>(),
  { size: 'md' },
)

const shown = computed(() => props.items.filter((item) => item.value != null))
</script>

<template>
  <dl v-if="shown.length" class="flex flex-wrap gap-x-8 gap-y-3">
    <div v-for="item in shown" :key="item.label">
      <dt class="text-muted-foreground text-xs tracking-wide uppercase">{{ item.label }}</dt>
      <dd class="font-semibold tabular-nums" :class="size === 'lg' ? 'text-2xl' : 'text-xl'">
        {{ item.value }}
      </dd>
    </div>
  </dl>
</template>
