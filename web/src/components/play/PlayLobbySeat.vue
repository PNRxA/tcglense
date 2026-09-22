<script setup lang="ts">
import { computed } from 'vue'
import { Check, Crown, UserRound, X } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import type { PlaySeatView } from '@/lib/api'

// One player in the lobby: who they are, whether they're here, and what they've brought.
//
// The three things that gate the start button are exactly the three things this row shows, so
// a host reading down the list can see what's missing without being told — a seat with no deck
// says so in the deck's place rather than leaving a blank line.
const props = defineProps<{
  seat: PlaySeatView
  /** True for the viewer's own seat — it reads "(you)" and can be acted on. */
  isMe: boolean
  /** The viewer hosts this room, so they may remove other seats. */
  canRemove?: boolean
  removing?: boolean
}>()

defineEmits<{ remove: [seatId: number] }>()

const deckLine = computed(() => {
  const { deck_name: name, deck_card_count: count } = props.seat
  if (!name) return null
  return count === null ? name : `${name} · ${count} cards`
})
</script>

<template>
  <li class="bg-card flex items-center gap-3 rounded-xl border p-3">
    <div class="bg-muted relative grid size-9 shrink-0 place-items-center rounded-lg">
      <Crown v-if="seat.is_host" class="size-5" aria-hidden="true" />
      <UserRound v-else class="size-5" aria-hidden="true" />
      <!-- A live socket, not a promise to show up: the lobby is where you find out someone's
           tab has died before you start the game rather than after. -->
      <span
        class="ring-card absolute -right-0.5 -bottom-0.5 size-2.5 rounded-full ring-2"
        :class="seat.connected ? 'bg-success' : 'bg-muted-foreground/40'"
        :title="seat.connected ? 'Connected' : 'Not connected'"
        :aria-label="seat.connected ? 'Connected' : 'Not connected'"
      />
    </div>

    <div class="min-w-0 flex-1">
      <p class="flex flex-wrap items-center gap-x-2 text-sm font-medium">
        <span class="truncate">{{ seat.display_name }}</span>
        <span v-if="isMe" class="text-muted-foreground font-normal">(you)</span>
        <span v-if="seat.is_host" class="text-muted-foreground font-normal">· host</span>
        <span v-if="!seat.is_user" class="text-muted-foreground font-normal">· guest</span>
      </p>
      <p v-if="deckLine" class="text-muted-foreground mt-0.5 truncate text-sm">{{ deckLine }}</p>
      <p v-else class="text-muted-foreground/80 mt-0.5 text-sm italic">No deck loaded yet</p>
      <p v-if="seat.commanders.length" class="text-muted-foreground mt-0.5 truncate text-xs">
        {{ seat.commanders.join(' + ') }}
      </p>
    </div>

    <span
      v-if="seat.ready"
      class="bg-success/15 text-success flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium"
    >
      <Check class="size-3" aria-hidden="true" /> Ready
    </span>
    <span
      v-else
      class="bg-muted text-muted-foreground shrink-0 rounded-full px-2 py-0.5 text-xs font-medium"
    >
      Not ready
    </span>

    <Button
      v-if="canRemove && !isMe"
      variant="ghost"
      size="icon-sm"
      :disabled="removing"
      :aria-label="`Remove ${seat.display_name}`"
      @click="$emit('remove', seat.id)"
    >
      <X class="size-4" aria-hidden="true" />
    </Button>
  </li>
</template>
