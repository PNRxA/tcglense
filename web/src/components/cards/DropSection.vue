<script setup lang="ts">
// One card-group section wrapper: an anchor-able, collapsible heading (group title + card
// count) above a default slot that holds the group's grid. Shared by the catalog set view
// and the collection/wish-list browse views (owned + ghost), across both groupings — Secret
// Lair drops and card sub-types (treatments) — which each render their own grid kind inside.
// The `v-for` + `:key` stay with the caller; this just wraps one group.
//
// An optional named `meta` slot renders trailing metadata pushed to the right of the heading
// (the catalog by-drop view fills it with each drop's "cheapest singles" total); it's absent
// everywhere else, so the wrapper only appears when a caller provides it.
//
// The heading is a disclosure toggle (same idiom as SetGroup / ProductCardsSection): open by
// default — the grouped view's sections ARE the primary listing, so they start expanded and
// collapse on demand. `open` is section-local; callers key each section on `<set>:<group>`, so
// switching sets (or the collection/wish-list ghost/owned mode) remounts the sections open,
// while a collapse only persists across a search/refetch within the same set. The grid stays
// mounted under `v-show` so a collapse/expand keeps its state (loaded images, hover) and just
// toggles `display`.
//
// `drop` is typed structurally (the fields every group DTO — `DropGroup`,
// `CollectionDropGroup`, `SubtypeGroup`, `CollectionSubtypeGroup` — shares), so any group
// shape works without coupling this to a card type. `PreconGroup` deliberately carries that
// same shape, but counts *decks* rather than cards and heads each set group with a link to
// that set's own page — hence `count`/`noun` and `to`, all optional and defaulting to the
// card-group behaviour every other caller relies on.
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { ChevronDown } from '@lucide/vue'

const props = withDefaults(
  defineProps<{
    drop: { slug: string | null; title: string; card_count?: number }
    // What the group holds, when it isn't the `card_count` a card group carries.
    count?: number
    // Singular noun for that count; pluralised with a bare "s".
    noun?: string
    // Makes the title a link to the group's own page (the precon by-set view's set pages).
    // The disclosure then shrinks to the chevron alone — a link nested inside a button is
    // neither valid HTML nor operable, and the title keeps the job it already had.
    to?: string
  }>(),
  { count: undefined, noun: 'card', to: undefined },
)

const open = ref(true)
const shownCount = computed(() => props.count ?? props.drop.card_count ?? 0)
const countLabel = computed(
  () => `${shownCount.value} ${shownCount.value === 1 ? props.noun : `${props.noun}s`}`,
)
</script>

<template>
  <section :id="drop.slug ?? undefined" class="mb-10 scroll-mt-20">
    <h2 class="mb-4 flex items-center gap-2 border-b pb-2 text-lg font-semibold tracking-tight">
      <div class="flex min-w-0 flex-1 items-center gap-2">
        <button
          type="button"
          class="group flex min-w-0 items-center gap-2 text-left"
          :class="to ? 'shrink-0' : 'flex-1'"
          :aria-expanded="open"
          :aria-label="to ? `${open ? 'Collapse' : 'Expand'} ${drop.title}` : undefined"
          @click="open = !open"
        >
          <ChevronDown
            class="text-muted-foreground group-hover:text-foreground size-5 shrink-0 transition-transform motion-reduce:transition-none"
            :class="open ? '' : '-rotate-90'"
          />
          <!-- Inside the toggle, so they read as its accessible name — unless the title is a
               link, which has to sit outside the button (see `to`). -->
          <template v-if="!to">
            <span class="truncate">{{ drop.title }}</span>
            <span class="text-muted-foreground text-sm font-normal tabular-nums">
              {{ countLabel }}
            </span>
          </template>
        </button>
        <template v-if="to">
          <RouterLink :to="to" class="truncate hover:underline">{{ drop.title }}</RouterLink>
          <span class="text-muted-foreground shrink-0 text-sm font-normal tabular-nums">
            {{ countLabel }}
          </span>
        </template>
      </div>
      <!-- Optional trailing metadata (e.g. the catalog by-drop view's "cheapest singles"
           total), right of the heading. Kept *beside* the toggle rather than inside it, so
           this live-updating, informational value isn't folded into the button's accessible
           name. Unfilled elsewhere, so the wrapper only renders when a caller provides it. -->
      <span v-if="$slots.meta" class="shrink-0">
        <slot name="meta" />
      </span>
    </h2>
    <div v-show="open">
      <slot />
    </div>
  </section>
</template>
