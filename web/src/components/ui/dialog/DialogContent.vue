<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { computed } from 'vue'
import { DialogContent, DialogPortal } from 'reka-ui'
import { cn } from '@/lib/utils'
import DialogOverlay from './DialogOverlay.vue'

const props = withDefaults(
  defineProps<{
    class?: HTMLAttributes['class']
    /**
     * Where the panel sits in the viewport.
     *
     * `'center'` (the default) is the shadcn placement: pinned to the viewport's midpoint,
     * so the panel grows equally in both directions.
     *
     * `'top'` pins the panel's TOP EDGE instead, and it is the right choice for any dialog
     * whose content arrives asynchronously or paginates. A centred panel re-centres itself
     * every time its height changes, which moves *everything already on screen* by half the
     * delta: a late-loading section at the bottom of the card modal silently slides the
     * collection steppers up under the user's finger, so a tap meant for "Regular +" lands
     * on "Foil +" a row below — one of the "sometimes my touch is off" mis-taps. It also
     * settles the panel on mobile, where a centred `position: fixed` box shifts whenever the
     * URL bar hides or reveals (the viewport it is centred in changes height; a pinned top
     * edge doesn't move). Growth then only ever extends downward, past content the user is
     * already looking at.
     *
     * Panels whose height is fixed by their content (a confirm prompt, the image lightbox)
     * have nothing to gain and stay centred.
     */
    anchor?: 'center' | 'top'
  }>(),
  { class: undefined, anchor: 'center' },
)

// Two-element template (overlay + content); forward stray attrs/listeners to the
// content rather than letting them land on the portal.
defineOptions({ inheritAttrs: false })

// A minimum inset keeps the top-anchored panel clear of the viewport edge on a short
// screen, where a percentage alone would leave it flush against the top.
const placement = computed(() =>
  props.anchor === 'top' ? 'top-[max(0.75rem,5svh)]' : 'top-1/2 -translate-y-1/2',
)
</script>

<template>
  <DialogPortal>
    <DialogOverlay />
    <!-- Structural only: placed + animated, no visual chrome — callers supply
      their own sizing/background via `class` (a frameless image lightbox here, a
      padded panel elsewhere). -->
    <DialogContent
      data-slot="dialog-content"
      :class="
        cn(
          'data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 fixed left-1/2 z-50 -translate-x-1/2 focus:outline-none motion-reduce:animate-none',
          placement,
          props.class,
        )
      "
      v-bind="$attrs"
    >
      <slot />
    </DialogContent>
  </DialogPortal>
</template>
