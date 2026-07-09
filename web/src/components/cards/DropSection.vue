<script setup lang="ts">
// One card-group section wrapper: an anchor-able heading (group title + card count) above
// a default slot that holds the group's grid. Shared by the catalog set view and the
// collection/wish-list browse views (owned + ghost), across both groupings — Secret Lair
// drops and card sub-types (treatments) — which each render their own grid kind inside.
// The `v-for` + `:key` stay with the caller; this just wraps one group.
//
// `drop` is typed structurally (the fields every group DTO — `DropGroup`,
// `CollectionDropGroup`, `SubtypeGroup`, `CollectionSubtypeGroup` — shares), so any group
// shape works without coupling this to a card type.
defineProps<{ drop: { slug: string | null; title: string; card_count: number } }>()
</script>

<template>
  <section :id="drop.slug ?? undefined" class="mb-10 scroll-mt-20">
    <div class="mb-4 flex items-baseline gap-2 border-b pb-2">
      <h2 class="text-lg font-semibold tracking-tight">{{ drop.title }}</h2>
      <span class="text-muted-foreground text-sm tabular-nums">
        {{ drop.card_count }} {{ drop.card_count === 1 ? 'card' : 'cards' }}
      </span>
    </div>
    <slot />
  </section>
</template>
