<script setup lang="ts">
import { computed } from 'vue'
import { BookOpen } from '@lucide/vue'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { ruleFor } from '@/lib/stack/rules'

// A Comprehensive Rules citation as a small chip that opens the plain-English version in
// place. A popover rather than a tooltip so it works the same for a mouse and a finger —
// the log is read on phones at the table as much as at a desk.
const props = defineProps<{ rule: string }>()
const rule = computed(() => ruleFor(props.rule))
</script>

<template>
  <Popover v-if="rule">
    <PopoverTrigger as-child>
      <button
        type="button"
        class="bg-info/15 text-info hover:bg-info/25 focus-visible:ring-ring/50 inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[0.7rem] font-medium tabular-nums transition-colors focus-visible:ring-2 focus-visible:outline-none"
        :aria-label="`Rule ${rule.id}: ${rule.title}`"
      >
        <BookOpen class="size-3" aria-hidden="true" />
        CR {{ rule.id }}
      </button>
    </PopoverTrigger>
    <PopoverContent :side-offset="6" class="w-80 p-3">
      <p class="text-muted-foreground text-[0.65rem] tracking-wide uppercase">
        Comprehensive Rules {{ rule.id }}
      </p>
      <p class="mt-0.5 font-medium">{{ rule.title }}</p>
      <p class="mt-1.5 text-sm leading-relaxed">{{ rule.text }}</p>
    </PopoverContent>
  </Popover>
</template>
