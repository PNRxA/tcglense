<script setup lang="ts">
import type { Component } from 'vue'

// The count-line cue: an inline icon followed by a label, sized and baseline-nudged to sit in a
// line of running text. A fragment with no wrapper of its own, so callers drop it straight into
// their count line and supply their own separators ("· ") and colour.
//
// It exists so the icon's metrics live in ONE place. `UpdatingCue` (refetch in flight) and the
// deck list's failed-refresh cue can share a count line, and a cue whose icon is a pixel bigger
// or a pixel higher than its neighbour reads as a rendering bug — `CardExportMenu`'s own
// hand-rolled inline TriangleAlert already drifted to `size-3 align-[-2px]`. Pass a different
// `icon` for a new state rather than re-rolling the class string.
//
// The icon is always decorative — `label` carries the whole meaning — so it's hidden from
// assistive tech here rather than at each call site. (Nothing else can be passed through: two
// root nodes means Vue has no single element to inherit attrs onto.)
defineProps<{ icon: Component; label: string; spin?: boolean }>()
</script>

<template>
  <component
    :is="icon"
    class="mr-1 inline size-3.5 align-[-0.15em]"
    :class="{ 'animate-spin': spin }"
    aria-hidden="true"
  />{{ label }}
</template>
