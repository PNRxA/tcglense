<script setup lang="ts">
// One card-group section wrapper: an anchor-able, collapsible heading (group title + card
// count) above a default slot that holds the group's grid. Shared by the catalog set view
// and the collection/wish-list browse views (owned + ghost), across both groupings — Secret
// Lair drops and card sub-types (treatments) — which each render their own grid kind inside.
// The `v-for` + `:key` stay with the caller; this just wraps one group.
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
// shape works without coupling this to a card type.
import { ref } from 'vue'
import { ChevronDown } from '@lucide/vue'

defineProps<{ drop: { slug: string | null; title: string; card_count: number } }>()

const open = ref(true)
</script>

<template>
  <section :id="drop.slug ?? undefined" class="mb-10 scroll-mt-20">
    <h2 class="mb-4 border-b pb-2 text-lg font-semibold tracking-tight">
      <button
        type="button"
        class="group flex w-full items-center gap-2 text-left"
        :aria-expanded="open"
        @click="open = !open"
      >
        <ChevronDown
          class="text-muted-foreground group-hover:text-foreground size-5 shrink-0 transition-transform motion-reduce:transition-none"
          :class="open ? '' : '-rotate-90'"
        />
        <span>{{ drop.title }}</span>
        <span class="text-muted-foreground text-sm font-normal tabular-nums">
          {{ drop.card_count }} {{ drop.card_count === 1 ? 'card' : 'cards' }}
        </span>
      </button>
    </h2>
    <div v-show="open">
      <slot />
    </div>
  </section>
</template>
