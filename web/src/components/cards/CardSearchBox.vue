<script setup lang="ts">
import { Search } from '@lucide/vue'
import { Input } from '@/components/ui/input'

// A search/filter text field with the magnifier icon overlaid in its left padding,
// shared by the catalog list views. `placeholder` and `ariaLabel` are threaded
// through per call site (SetView varies its placeholder with the scope); width is
// passed through via a fall-through `class` onto the relative frame.
defineProps<{
  placeholder: string
  ariaLabel?: string
}>()

// The live text box; the parent owns it (e.g. useCardSearch's debounced input).
const model = defineModel<string>({ required: true })
</script>

<template>
  <div class="relative">
    <Search
      class="text-muted-foreground pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2"
    />
    <Input v-model="model" :placeholder="placeholder" :aria-label="ariaLabel" class="pl-9" />
  </div>
</template>
