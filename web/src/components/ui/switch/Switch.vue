<script setup lang="ts">
// A small on/off switch (issue #381) — shared by the collection settings menu's display
// toggles and the sharing (public/private) control. Deliberately *controlled* (a `checked`
// prop + an `update:checked` event, so it works as `v-model:checked`) rather than holding
// its own state via defineModel: the sharing switch's parent may reject or defer a toggle
// (clicking it can open a "choose a username" dialog instead of flipping), so the switch
// must always reflect the parent's `checked` and never an optimistic guess. Hand-written
// rather than a shadcn/reka primitive to reuse the switch styling already used across the
// app: a plain button with role="switch", so it needs no extra dependency. The visible
// label stays with the caller, which passes `aria-label`/`aria-labelledby` through.
defineProps<{ checked: boolean; disabled?: boolean }>()
const emit = defineEmits<{ 'update:checked': [boolean] }>()
</script>

<template>
  <button
    type="button"
    role="switch"
    :aria-checked="checked"
    :disabled="disabled"
    class="focus-visible:ring-ring/50 inline-flex h-6 w-11 shrink-0 items-center rounded-full border transition-colors outline-none focus-visible:ring-2 disabled:cursor-not-allowed disabled:opacity-50"
    :class="checked ? 'bg-primary border-primary' : 'bg-input border-input'"
    @click="emit('update:checked', !checked)"
  >
    <span
      class="bg-background size-5 rounded-full shadow-sm transition-transform"
      :class="checked ? 'translate-x-5' : 'translate-x-0.5'"
    />
  </button>
</template>
