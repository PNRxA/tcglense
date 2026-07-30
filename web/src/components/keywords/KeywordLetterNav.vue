<script setup lang="ts">
// The A–Z jump strip above the glossary index. Letters with no entries under the
// current filter stay in place, dimmed and inert, so the strip keeps its shape rather
// than reflowing as you type.
//
// These are buttons doing `scrollIntoView`, deliberately not `#letter` hash links: the
// router's `scrollBehavior` returns `false` for a navigation to the same path, so a hash
// link here would update the URL and then not scroll.
defineProps<{ letters: { letter: string; id: string; present: boolean }[] }>()

function jump(id: string) {
  document.getElementById(id)?.scrollIntoView({ block: 'start' })
}
</script>

<template>
  <nav aria-label="Jump to letter" class="flex flex-wrap gap-0.5">
    <template v-for="entry in letters" :key="entry.letter">
      <button
        v-if="entry.present"
        type="button"
        class="hover:bg-accent hover:text-accent-foreground w-7 rounded py-1 text-sm font-medium transition-colors"
        @click="jump(entry.id)"
      >
        {{ entry.letter }}
      </button>
      <span
        v-else
        aria-hidden="true"
        class="text-muted-foreground w-7 py-1 text-center text-sm opacity-40"
      >
        {{ entry.letter }}
      </span>
    </template>
  </nav>
</template>
