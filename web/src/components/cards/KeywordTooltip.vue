<script setup lang="ts">
import { onScopeDispose, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { KIND_LABELS } from '@/lib/keywords'
import type { KeywordEntry } from '@/lib/api'

// One keyword inside a card's rules text, turned into an explanation you can reach
// without leaving the card.
//
// **Why two primitives.** A tooltip is the right desktop affordance, but reka's opens
// on hover and focus only and dismisses on pointerdown — on a touch screen it is
// effectively dead. So a coarse pointer gets a Popover instead, which opens on tap and
// closes on Escape or an outside tap, and carries a link on to the full glossary entry
// (a phone has no "hover to peek, click to read" split to lean on). The two branches
// are deliberately different elements, because the right trigger differs: a link that
// navigates on click for a mouse, a button that discloses for a finger.
//
// The trigger is a real `<button>`/`<a>` rather than a `<span>`, so it is focusable and
// operable from the keyboard for free — the cost is one tab stop per distinct keyword,
// which is the honest price of the feature being reachable at all.
const props = defineProps<{
  /** The glossary entry this run of text refers to. */
  entry: KeywordEntry
  /** The matched text exactly as the card spells it, so casing survives. */
  label: string
}>()

/** Whether the device can hover — decided once per document and shared by every
 * instance, since a card's rules text mounts one of these per keyword. A media query
 * (not a touch-events sniff) so a hybrid laptop follows whichever input is in use, and
 * so it re-evaluates if the user picks up a pen or plugs in a mouse. */
const HOVER_QUERY = '(hover: hover) and (pointer: fine)'
const canHover = ref(true)
if (typeof window !== 'undefined' && typeof window.matchMedia === 'function') {
  const media = window.matchMedia(HOVER_QUERY)
  canHover.value = media.matches
  const onChange = (event: MediaQueryListEvent) => {
    canHover.value = event.matches
  }
  media.addEventListener('change', onChange)
  onScopeDispose(() => media.removeEventListener('change', onChange))
}

const open = ref(false)

/** Styling for the inline marker. Deliberately decoration-only — no padding, border or
 * `display` change — because it sits in a `whitespace-pre-line` paragraph where any box
 * of its own would break the line rhythm, and a multi-word keyword must still be able
 * to wrap. */
const TRIGGER_CLASS =
  'cursor-help rounded-sm underline decoration-dotted decoration-muted-foreground/70 ' +
  'underline-offset-4 transition-colors hover:decoration-foreground ' +
  'focus-visible:ring-ring/50 focus-visible:ring-2 focus-visible:outline-none'
</script>

<template>
  <!-- Both branches are written hugged (`><`, `/><`) with no whitespace between the
    trigger and the surrounding card text: the parent paragraph is `whitespace-pre-line`,
    where a newline in the template would render as a visible space mid-sentence. -->
  <Tooltip v-if="canHover"
    ><TooltipTrigger as-child
      ><RouterLink :to="`/keywords/${props.entry.slug}`" :class="TRIGGER_CLASS">{{
        props.label
      }}</RouterLink></TooltipTrigger
    >
    <TooltipContent :side-offset="6" class="max-w-xs">
      <p class="text-[0.65rem] tracking-wide uppercase opacity-70">
        {{ KIND_LABELS[props.entry.kind] }}
      </p>
      <p class="mt-0.5 leading-relaxed">{{ props.entry.text }}</p>
    </TooltipContent>
  </Tooltip>
  <Popover v-else v-model:open="open"
    ><PopoverTrigger as-child
      ><button type="button" :class="TRIGGER_CLASS">{{ props.label }}</button></PopoverTrigger
    >
    <PopoverContent :side-offset="6" class="w-72 p-3">
      <p class="text-muted-foreground text-[0.65rem] tracking-wide uppercase">
        {{ KIND_LABELS[props.entry.kind] }}
      </p>
      <p class="mt-0.5 font-medium">{{ props.entry.name }}</p>
      <p class="mt-1.5 text-sm leading-relaxed">{{ props.entry.text }}</p>
      <RouterLink
        :to="`/keywords/${props.entry.slug}`"
        class="text-primary mt-3 inline-block text-sm font-medium hover:underline"
        @click="open = false"
      >
        Full entry &rarr;
      </RouterLink>
    </PopoverContent>
  </Popover>
</template>
