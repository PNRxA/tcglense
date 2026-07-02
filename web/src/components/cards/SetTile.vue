<script setup lang="ts">
import { computed } from 'vue'
import { Layers } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { setIconUrl, type CardSet } from '@/lib/api'
import { formatCompletion, formatCopies } from '@/lib/ownership'
import { useImageLoad } from '@/composables/useImageLoad'

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
    // Optional link-target override. Defaults to the catalog set page; the
    // collection landing points it at its own per-set view instead.
    to?: string
    // Optional owned-card count. When set, the meta line shows a set-completion
    // "N/M owned" count (M = the set's total card count) in place of the set's total
    // card count — the collection's per-set landing (issue #125).
    ownedCount?: number
    // Optional total owned copies (regular + foil, i.e. counting duplicates). Shown as
    // "· N copies" after the completion count, but only when it exceeds the distinct owned
    // count — with no duplicates the completion count already conveys the total (issue #125).
    ownedCopies?: number
    // Optional preformatted owned value (e.g. "$123.45"). When set, it's shown as
    // "TOTAL $X" on the identity line (in line with the release date) — the collection
    // landing's per-set total value.
    ownedValue?: string | null
    // Optional preformatted bulk (< $1/card) value (e.g. "$12.30"). When set, it's shown
    // as "BULK $X" on the stats line (in line with the owned count) — the collection
    // landing's per-set bulk value.
    bulkValue?: string | null
  }>(),
  {
    variant: 'default',
    label: undefined,
    to: undefined,
    ownedCount: undefined,
    ownedCopies: undefined,
    ownedValue: undefined,
    bulkValue: undefined,
  },
)

const nested = computed(() => props.variant === 'nested')
const displayName = computed(() => props.label ?? props.set.name)
// Where the tile links: an explicit override (collection view) or the catalog set page.
const linkTo = computed(() => props.to ?? `/cards/${props.game}/sets/${props.set.code}`)
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
// graceful fallback if the fetch fails. SetTile instances are reused across a set
// list (v-for), so the load state resets when we point at a different set's icon.
const {
  el: iconEl,
  loaded: iconLoaded,
  failed: iconFailed,
  onLoad,
  onError,
} = useImageLoad(() => [props.game, props.set.code])
const showIcon = computed(() => !!props.set.icon_svg_uri && !iconFailed.value)

const released = computed(() => {
  if (!props.set.released_at) return null
  const date = new Date(props.set.released_at)
  if (Number.isNaN(date.getTime())) return props.set.released_at
  return date.toLocaleDateString(undefined, { year: 'numeric', month: 'short' })
})

// The owned-count line on the collection landing: a set-completion "N/M owned" count
// (owned / the set's total card count) so how much of the set you have reads at a glance
// (issue #125). `ownedCount` is clamped to the total so a paper-only vs. Scryfall
// card-count skew can never read "N+1 of N"; when the total is unknown (card_count 0) it
// degrades to a plain "N owned".
const ownedLabel = computed(() => {
  if (props.ownedCount == null) return null
  const total = props.set.card_count
  if (total > 0) return formatCompletion(props.ownedCount, total)
  return `${props.ownedCount.toLocaleString()} owned`
})

// The total copies (with duplicates) as "N copies", shown next to the completion count
// only when you own more copies than distinct cards — otherwise it just restates the
// owned count (issue #125).
const copiesLabel = computed(() =>
  props.ownedCount != null && props.ownedCopies != null && props.ownedCopies > props.ownedCount
    ? formatCopies(props.ownedCopies)
    : null,
)
</script>

<template>
  <RouterLink
    :to="linkTo"
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
          @load="onLoad"
          @error="onError"
        />
        <Layers v-else class="text-muted-foreground" :class="nested ? 'size-5' : 'size-6'" />
      </div>
      <div class="min-w-0 flex-1">
        <p class="truncate font-medium" :class="nested ? 'text-sm' : ''" :title="set.name">
          {{ displayName }}
        </p>
        <!-- Set identity (code + release). Off the collection landing the set's total card
             count rides along here; on it, the owned counts move to their own line below so
             the combined string can't overflow this one (issue #125). The collection
             landing also pins the total owned value ("TOTAL $X") to the right here, in line
             with the release date, so the identity truncates first and the worth stays. -->
        <div class="text-muted-foreground flex items-baseline justify-between gap-2 text-xs">
          <span class="min-w-0 truncate">
            {{ set.code.toUpperCase() }}
            <template v-if="released"> · {{ released }}</template>
            <template v-if="ownedLabel == null && set.card_count">
              · {{ set.card_count }} cards</template
            >
          </span>
          <span
            v-if="ownedValue"
            class="text-foreground shrink-0 font-medium tabular-nums"
            title="Total estimated value"
          >
            <span class="text-muted-foreground text-[0.625rem] tracking-wide uppercase">Total</span>
            {{ ownedValue }}
          </span>
        </div>
        <!-- Collection stats on their own line: the completion count (+ copies) truncates
             first while the bulk value stays pinned to the right, in line with the count, so
             the worth is never the bit that gets clipped on a narrow tile. -->
        <div
          v-if="ownedLabel != null"
          class="text-muted-foreground mt-0.5 flex items-baseline justify-between gap-2 text-xs"
        >
          <span class="min-w-0 truncate tabular-nums">
            {{ ownedLabel }}<template v-if="copiesLabel"> · {{ copiesLabel }}</template>
          </span>
          <span
            v-if="bulkValue"
            class="text-foreground shrink-0 font-medium tabular-nums"
            title="Value of cards worth under $1 each"
          >
            <span class="text-muted-foreground text-[0.625rem] tracking-wide uppercase">Bulk</span>
            {{ bulkValue }}
          </span>
        </div>
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
