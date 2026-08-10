<script setup lang="ts">
import { computed } from 'vue'
import { MonitorSmartphone } from '@lucide/vue'

// Whether the screen is being kept awake — said honestly.
//
// The Screen Wake Lock API isn't everywhere (Firefox, older iOS Safari), and a request can be
// refused. Claiming "screen stays on" when it won't is worse than saying nothing, so this pill
// reports what is actually true: held, not held, or unsupported here.
const props = defineProps<{ supported: boolean; active: boolean }>()

const copy = computed(() => {
  if (!props.supported) {
    return {
      label: 'Screen may sleep',
      title: "This browser can't keep the screen awake. Adjust your device's screen timeout.",
      tone: 'text-muted-foreground',
    }
  }
  return props.active
    ? {
        label: 'Screen staying on',
        title: 'The screen is being kept awake while this game is in progress.',
        tone: 'text-success',
      }
    : {
        label: 'Screen may sleep',
        title: 'The screen lock is not being held right now.',
        tone: 'text-muted-foreground',
      }
})
</script>

<template>
  <span
    class="inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-xs font-medium"
    :class="[copy.tone, active && supported ? 'bg-success/15' : 'bg-muted']"
    :title="copy.title"
  >
    <MonitorSmartphone class="size-3.5" aria-hidden="true" />
    {{ copy.label }}
  </span>
</template>
