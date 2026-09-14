<script setup lang="ts">
import { computed, ref } from 'vue'
import { Eye, UserRound } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import type { PlayRoomSummary } from '@/lib/api'

// The door: what a viewer holding no seat sees.
//
// A guest gives a name and nothing else — no account, no email, no verification step — because
// every extra field here is a friend who doesn't make it to the table. A signed-in visitor
// doesn't even get the field: the server names their seat from their username, so the whole
// interaction is one button.
//
// A room that is full or already playing is a dead end for *joining* but not for *watching*, so
// it says which of the two it is and offers the spectator view instead of an error.
const props = defineProps<{
  room: PlayRoomSummary
  /** Signed-in visitors join with one button; guests need a name. */
  isAuthenticated: boolean
  busy?: boolean
  error?: string | null
}>()

const emit = defineEmits<{ join: [name: string]; watch: [] }>()

const name = ref('')
const MAX_NAME = 32

const isFull = computed(() => props.room.seats.length >= props.room.max_players)
const hasStarted = computed(() => props.room.status !== 'lobby')
const canJoin = computed(() => !isFull.value && !hasStarted.value)
const canSubmit = computed(() => props.isAuthenticated || name.value.trim().length > 0)

const closedReason = computed(() => {
  if (hasStarted.value) {
    return props.room.status === 'finished'
      ? 'This game has finished.'
      : 'This game has already started.'
  }
  return 'Every seat at this table is taken.'
})

function submit() {
  if (!canSubmit.value || props.busy) return
  emit('join', name.value.trim())
}
</script>

<template>
  <div class="bg-card mx-auto max-w-md rounded-xl border p-6">
    <div class="bg-muted mx-auto grid size-12 place-items-center rounded-lg">
      <UserRound v-if="canJoin" class="size-6" aria-hidden="true" />
      <Eye v-else class="size-6" aria-hidden="true" />
    </div>

    <h1 class="mt-4 text-center text-xl font-semibold tracking-tight">
      {{ canJoin ? room.label || 'Join the table' : 'This table is closed' }}
    </h1>
    <p class="text-muted-foreground mt-2 text-center text-sm capitalize">
      {{ room.format }} · {{ room.starting_life }} life · {{ room.seats.length }}/{{
        room.max_players
      }}
      seated
    </p>

    <template v-if="canJoin">
      <p v-if="room.seats.length" class="text-muted-foreground mt-3 text-center text-sm">
        Already here: {{ room.seats.map((seat) => seat.display_name).join(', ') }}
      </p>

      <form class="mt-5 space-y-3" @submit.prevent="submit">
        <template v-if="!isAuthenticated">
          <Label for="play-join-name">Your name</Label>
          <Input
            id="play-join-name"
            v-model="name"
            :maxlength="MAX_NAME"
            autocomplete="nickname"
            placeholder="What should we call you?"
          />
          <p class="text-muted-foreground text-xs">
            No account needed — this is just what the other players see.
          </p>
        </template>
        <Button type="submit" class="w-full" :disabled="!canSubmit || busy">Take a seat</Button>
      </form>
    </template>

    <template v-else>
      <p class="text-muted-foreground mt-4 text-center text-sm">{{ closedReason }}</p>
      <Button variant="outline" class="mt-4 w-full" :disabled="busy" @click="emit('watch')">
        <Eye class="size-4" aria-hidden="true" /> Watch instead
      </Button>
    </template>

    <p v-if="error" class="text-destructive mt-3 text-center text-sm">{{ error }}</p>
  </div>
</template>
