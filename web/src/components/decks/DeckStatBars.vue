<script setup lang="ts">
import { computed } from 'vue'
import type { DeckStatItem } from '@/lib/api'

// One distribution from the deck's composition, drawn either way round.
//
// `rows` (the default) is the analytics panel's expanded body: a labelled bar per bucket with
// its exact count beside it. `columns` is the same distribution as a compact strip — the
// collapsed panel's mana curve, where the shape is the whole point and there is no room for
// eight labelled rows.
//
// Both live here rather than in a second component because the collapsed strip is the summary
// *of* the rows the expanded body draws, and the two must never disagree: the scale
// (`maximum`), the `item.color ?? var(--primary)` fallback and the per-bucket accessible label
// are exactly the things a fork drifts on.
//
// `selectable` (issue #671, the card-roles panel) makes each `rows` bucket a toggle bound to
// `v-model:selected` — the bar and the control it doubles as are the same object, so a role's
// size and the way to filter by it can't drift apart. It changes the `rows` markup ONLY while
// it is on: the row's element is a `button` instead of a `div` and nothing else moves, so a
// plain bar chart still renders exactly what it always did (`columns` ignores the prop
// entirely — a 30px-wide column is not a hit target).
const props = withDefaults(
  defineProps<{
    title: string
    items: DeckStatItem[]
    /** `columns` renders the compact strip. Only fits **short** bucket labels (the mana
     * curve's `0`..`7+`) — a colour or card-type label under a ~30px column would truncate to
     * two characters, so those stay in `rows`. */
    layout?: 'rows' | 'columns'
    /** Turn each `rows` bucket into a toggle for `v-model:selected`. Ignored by `columns`. */
    selectable?: boolean
  }>(),
  { layout: 'rows', selectable: false },
)

/** The selected bucket's `key`, or null for none. Clicking the selected one clears it, so
 * the filter a bar stands for is always undone by the same bar that set it. */
const selected = defineModel<string | null>('selected', { default: null })

function toggle(key: string) {
  if (!props.selectable) return
  selected.value = selected.value === key ? null : key
}
const maximum = computed(() => Math.max(1, ...props.items.map((item) => item.count)))

/** This bucket's share of the tallest one — the bar's length in either orientation. */
function share(count: number): string {
  return `${(count / maximum.value) * 100}%`
}

/** "3: 12 copies" — one wording for a bucket, so a bar reads the same in both layouts. */
function barLabel(item: DeckStatItem): string {
  return `${item.label}: ${item.count} ${item.count === 1 ? 'copy' : 'copies'}`
}
</script>

<template>
  <!-- The compact strip. Every bucket keeps a visible track, so eight buckets stay eight
    buckets, and only a bucket with copies in it gets a fill — the rows layout's unconditional
    `min-w-px` floor would paint a hairline for a zero, which is harmless beside a printed
    count and a false claim in a strip that prints none. The track carries the label because
    the fill is zero-height exactly when there is nothing to announce. -->
  <section v-if="layout === 'columns'">
    <h3 class="text-muted-foreground mb-1.5 text-[0.7rem] font-medium tracking-wide uppercase">
      {{ title }}
    </h3>
    <div class="grid auto-cols-fr grid-flow-col gap-1">
      <div v-for="item in items" :key="item.key">
        <!-- Taller than the columns are wide, so the distribution reads as a chart rather
          than a row of pills. Callers cap the strip's width for the same reason (attribute
          fallthrough lands on this section) — stretched across a desktop card, eight buckets
          are 130px wide and 36px tall, which is not a curve. -->
        <div
          class="bg-muted flex h-12 flex-col justify-end overflow-hidden rounded-sm"
          role="img"
          :aria-label="barLabel(item)"
        >
          <div
            class="rounded-sm motion-safe:transition-[height]"
            :class="item.count > 0 ? 'min-h-0.5' : ''"
            :style="{ height: share(item.count), backgroundColor: item.color ?? 'var(--primary)' }"
          />
        </div>
        <p
          class="text-muted-foreground mt-1 text-center text-[0.65rem] tabular-nums"
          aria-hidden="true"
        >
          {{ item.label }}
        </p>
      </div>
    </div>
  </section>

  <!-- The labelled rows. The row element is a `button` only when the caller asked for one:
    an unselectable chart must render exactly the markup it rendered before this prop existed,
    which is why this is one `:is` rather than two copies of the bar. -->
  <section v-else>
    <h3 class="mb-3 text-sm font-semibold">{{ title }}</h3>
    <div class="space-y-2">
      <component
        :is="selectable ? 'button' : 'div'"
        v-for="item in items"
        :key="item.key"
        :type="selectable ? 'button' : undefined"
        :aria-pressed="selectable ? selected === item.key : undefined"
        :aria-label="selectable ? barLabel(item) : undefined"
        :class="
          selectable
            ? [
                'focus-visible:ring-ring/50 block w-full rounded-md px-1.5 py-1 text-left outline-none focus-visible:ring-3',
                selected === item.key ? 'ring-ring/50 bg-muted/60 ring-2' : 'hover:bg-muted/40',
              ]
            : undefined
        "
        @click="toggle(item.key)"
      >
        <div class="mb-1 flex items-center justify-between gap-3 text-xs">
          <span>{{ item.label }}</span>
          <span class="text-muted-foreground tabular-nums">{{ item.count }}</span>
        </div>
        <div class="bg-muted h-2 overflow-hidden rounded-full">
          <!-- As a button the row carries the label itself; the fill inside is then hidden
            from the accessibility tree so the same words aren't announced twice. -->
          <div
            class="h-full min-w-px rounded-full transition-[width]"
            :style="{ width: share(item.count), backgroundColor: item.color ?? 'var(--primary)' }"
            role="img"
            :aria-label="barLabel(item)"
            :aria-hidden="selectable || undefined"
          />
        </div>
      </component>
    </div>
  </section>
</template>
