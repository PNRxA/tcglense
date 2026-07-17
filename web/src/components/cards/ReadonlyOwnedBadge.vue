<script setup lang="ts">
import { computed } from 'vue'
import OwnedCountBadge from '@/components/cards/OwnedCountBadge.vue'

// A read-only owned-count badge for a card tile — the static, non-interactive counterpart of
// OwnedCountControl's trigger. Used by the public (read-only) collection browse grids, where a
// viewer must never get an editor: it shows the OWNER's owned counts (a total chip, plus a
// foil chip when any copies are foil, via OwnedCountBadge) anchored bottom-left over the art,
// matching the quick-add control's placement (issue #100) so the public grid's layout is
// pixel-identical to the authed one. Renders nothing on an unowned card (total 0). Tooltip is
// off (matching the authed resting badge) so it needs no TooltipProvider ancestor — the count
// chips carry their own `aria-label` for screen readers.
const props = defineProps<{ quantity: number; foilQuantity: number }>()
const total = computed(() => props.quantity + props.foilQuantity)
</script>

<template>
  <span v-if="total > 0" class="absolute bottom-1.5 left-1.5 z-20 inline-flex items-center">
    <OwnedCountBadge
      :quantity="quantity"
      :foil-quantity="foilQuantity"
      kind="owned"
      :tooltip="false"
    />
  </span>
</template>
