<script setup lang="ts">
import { computed, ref } from 'vue'
import { ChevronRight, Trash2 } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { Button } from '@/components/ui/button'
import type { PlayRoomSummary, PlayRoomStatus } from '@/lib/api'
import { playRoomPath } from '@/lib/tools'

// One of your rooms in the hub's list: enough to recognise it, and the button you came for.
//
// The action label tracks the status because "Resume" and "Open" mean different things to
// someone scanning the list — a game in progress is the row you're almost certainly here for,
// which is also why the hub sorts those to the top.
const props = defineProps<{
  room: PlayRoomSummary
  game: string
  /** Show the host's close action. The server is the authority; this only hides a button. */
  canDelete?: boolean
  deleting?: boolean
}>()

const emit = defineEmits<{ delete: [code: string] }>()

const STATUS_CHIPS: Record<PlayRoomStatus, { label: string; classes: string }> = {
  playing: { label: 'In progress', classes: 'bg-success/15 text-success' },
  lobby: { label: 'In the lobby', classes: 'bg-info/15 text-info' },
  finished: { label: 'Finished', classes: 'bg-muted text-muted-foreground' },
}

const chip = computed(() => STATUS_CHIPS[props.room.status])
const actionLabel = computed(() => (props.room.status === 'playing' ? 'Resume' : 'Open'))
const seatLine = computed(() =>
  props.room.seats.length
    ? props.room.seats.map((seat) => seat.display_name).join(', ')
    : 'No one has taken a seat yet',
)

const confirming = ref(false)
</script>

<template>
  <div class="bg-card flex flex-wrap items-center gap-3 rounded-xl border p-4">
    <div class="min-w-0 flex-1">
      <p class="flex flex-wrap items-center gap-2 font-medium">
        <span class="truncate">{{ room.label || 'Untitled room' }}</span>
        <span class="text-muted-foreground font-mono text-xs tracking-widest">{{ room.code }}</span>
        <span class="shrink-0 rounded-full px-2 py-0.5 text-xs font-medium" :class="chip.classes">{{
          chip.label
        }}</span>
      </p>
      <p class="text-muted-foreground mt-1 truncate text-sm capitalize">
        {{ room.format }} · {{ room.seats.length }}/{{ room.max_players }} players
      </p>
      <p class="text-muted-foreground mt-0.5 truncate text-sm">{{ seatLine }}</p>
    </div>

    <template v-if="confirming">
      <p class="text-sm">Close it for everyone?</p>
      <Button
        variant="destructive"
        size="sm"
        :disabled="deleting"
        @click="emit('delete', room.code)"
      >
        Close room
      </Button>
      <Button variant="outline" size="sm" @click="confirming = false">Cancel</Button>
    </template>

    <template v-else>
      <RouterLink :to="playRoomPath(game, room.code)">
        <Button variant="outline" size="sm">
          {{ actionLabel }} <ChevronRight class="size-4" aria-hidden="true" />
        </Button>
      </RouterLink>
      <Button
        v-if="canDelete"
        variant="ghost"
        size="icon-sm"
        :aria-label="`Close room ${room.code}`"
        @click="confirming = true"
      >
        <Trash2 class="size-4" aria-hidden="true" />
      </Button>
    </template>
  </div>
</template>
