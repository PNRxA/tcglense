<script setup lang="ts">
import { ChevronDown } from '@lucide/vue'

// The shared disclosure block for the detail pages' heavy sections — a card's other
// printings and sealed-product buckets, a sealed product's per-membership card sections.
// One idiom (a bordered card whose header shows the title + count + one-line blurb and
// toggles the body) instead of three hand-rolled copies. Collapsed by default via the
// model default; the body only mounts while expanded, so callers can gate their queries
// on the bound state and a collapsed block costs nothing (issues #291/#332).
withDefaults(
  defineProps<{
    title: string
    /** Item count beside the title, so a collapsed block still says how much it hides. */
    count?: number | null
    /** One-line explanation under the title (e.g. how strong a membership claim is). */
    blurb?: string
    /** Heading level for the title — h2 for page-level sections, h3 for nested buckets. */
    heading?: 'h2' | 'h3'
  }>(),
  { count: null, blurb: '', heading: 'h3' },
)

const expanded = defineModel<boolean>('expanded', { default: false })
</script>

<template>
  <section class="bg-card rounded-xl border shadow-sm">
    <button
      type="button"
      class="group hover:bg-muted/40 flex w-full items-center gap-3 rounded-xl px-4 py-3 text-left transition-colors"
      :class="expanded ? 'rounded-b-none' : ''"
      :aria-expanded="expanded"
      @click="expanded = !expanded"
    >
      <span class="min-w-0 flex-1">
        <component :is="heading" class="text-sm font-semibold">
          {{ title }}
          <span v-if="count != null" class="text-muted-foreground font-normal"
            >({{ count.toLocaleString() }})</span
          >
        </component>
        <span v-if="blurb" class="text-muted-foreground mt-0.5 block text-xs">{{ blurb }}</span>
      </span>
      <ChevronDown
        class="text-muted-foreground group-hover:text-foreground size-4 shrink-0 transition-transform"
        :class="expanded ? 'rotate-180' : ''"
        aria-hidden="true"
      />
    </button>
    <div v-if="expanded" class="border-t px-4 py-4">
      <slot />
    </div>
  </section>
</template>
