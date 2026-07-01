<script setup lang="ts">
// One Secret Lair drop's section wrapper: an anchor-able heading (drop title + card
// count) above a default slot that holds the drop's grid. Shared by the catalog set
// view and the collection browse view (owned + ghost), which each render their own grid
// kind inside. The `v-for` + `:key` stay with the caller; this just wraps one drop.
//
// `drop` is typed structurally (the fields both `DropGroup` and `CollectionDropGroup`
// share), so either drop shape works without coupling this to a card type.
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
