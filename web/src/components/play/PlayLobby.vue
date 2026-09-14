<script setup lang="ts">
import { computed, ref } from 'vue'
import { LogOut, Play, Trash2 } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import PlayDeckPicker from '@/components/play/PlayDeckPicker.vue'
import PlayInviteLink from '@/components/play/PlayInviteLink.vue'
import PlayLobbySeat from '@/components/play/PlayLobbySeat.vue'
import { useSetPlaySeatReady } from '@/composables/usePlayRooms'
import { PLAY_DEFAULT_LIFE, type PlayFormat } from '@/lib/api/play'
import type { PlayRoomSummary, PlaySeatView } from '@/lib/api'

// The room before the game: who's here, what they've brought, and the one button that turns it
// into a table.
//
// The start button is the whole point of this screen, so it never just sits there disabled: when
// it can't be pressed it says which of the two conditions isn't met (a second player, or a deck
// in every seat). A host staring at a greyed-out button with no explanation is the failure mode
// this replaces — and both conditions are the server's, so the message here is a courtesy, not
// the enforcement.
const props = defineProps<{
  game: string
  code: string
  room: PlayRoomSummary
  /** The viewer's seat, or null when they're watching. */
  mySeat: PlaySeatView | null
  seatToken: string | null
  starting?: boolean
  leaving?: boolean
  closing?: boolean
}>()

const emit = defineEmits<{ start: []; leave: []; close: []; remove: [seatId: number] }>()

const seats = computed(() => props.room.seats)
const isHost = computed(() => props.mySeat?.is_host ?? false)
const withoutDeck = computed(() => seats.value.filter((seat) => !seat.deck_name))

/** What stops the game starting, in the order a host would fix it. */
const blocker = computed<string | null>(() => {
  if (seats.value.length < 2) return 'Waiting for at least one more player to take a seat.'
  if (withoutDeck.value.length) {
    const names = withoutDeck.value.map((seat) => seat.display_name).join(', ')
    return `Still without a deck: ${names}.`
  }
  return null
})

const canStart = computed(() => blocker.value === null)

const startingLifeLine = computed(() => {
  const fallback = PLAY_DEFAULT_LIFE[props.room.format as PlayFormat]
  const custom = fallback !== undefined && fallback !== props.room.starting_life
  return custom ? `${props.room.starting_life} life (custom)` : `${props.room.starting_life} life`
})

const ready = useSetPlaySeatReady()

async function toggleReady() {
  const seat = props.mySeat
  if (!seat || !props.seatToken) return
  await ready.mutateAsync({
    game: props.game,
    code: props.code,
    seatId: seat.id,
    seatToken: props.seatToken,
    ready: !seat.ready,
  })
}

/** Closing a room ends it for everyone mid-lobby, so it asks first. */
const confirmingClose = ref(false)
</script>

<template>
  <div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_22rem]">
    <div class="space-y-4">
      <header class="bg-card rounded-xl border p-4">
        <h1 class="text-xl font-semibold tracking-tight">
          {{ room.label || 'Play online' }}
        </h1>
        <p class="text-muted-foreground mt-1 text-sm capitalize">
          {{ room.format }} · {{ startingLifeLine }} · {{ seats.length }}/{{ room.max_players }}
          seated
        </p>
      </header>

      <section>
        <h2 class="mb-2 text-sm font-medium">At the table</h2>
        <ul class="space-y-2">
          <PlayLobbySeat
            v-for="seat in seats"
            :key="seat.id"
            :seat="seat"
            :is-me="seat.id === mySeat?.id"
            :can-remove="isHost"
            @remove="emit('remove', $event)"
          />
        </ul>
        <p v-if="!mySeat" class="text-muted-foreground mt-2 text-sm">
          You're watching this room — you'll see the table when the host starts.
        </p>
      </section>

      <PlayDeckPicker
        v-if="mySeat && seatToken"
        :game="game"
        :code="code"
        :seat-id="mySeat.id"
        :seat-token="seatToken"
        :current-deck-name="mySeat.deck_name"
      />
    </div>

    <aside class="space-y-4">
      <PlayInviteLink :game="game" :code="code" />

      <div v-if="mySeat" class="bg-card space-y-3 rounded-xl border p-4">
        <Button
          class="w-full"
          :variant="mySeat.ready ? 'outline' : 'default'"
          :disabled="!mySeat.deck_name || ready.isPending.value"
          @click="toggleReady"
        >
          {{ mySeat.ready ? "I'm not ready" : "I'm ready" }}
        </Button>
        <p v-if="!mySeat.deck_name" class="text-muted-foreground text-xs">
          Load a deck first — readying without one won't stick.
        </p>
        <p v-if="ready.error.value" class="text-destructive text-xs">
          {{ ready.error.value.message }}
        </p>

        <template v-if="isHost">
          <Button class="w-full" :disabled="!canStart || starting" @click="emit('start')">
            <Play class="size-4" aria-hidden="true" /> Start the game
          </Button>
          <p v-if="blocker" class="text-muted-foreground text-xs" data-testid="play-start-blocker">
            {{ blocker }}
          </p>
        </template>
      </div>

      <div class="bg-card rounded-xl border p-4">
        <template v-if="isHost">
          <Button
            v-if="!confirmingClose"
            variant="outline"
            class="w-full"
            @click="confirmingClose = true"
          >
            <Trash2 class="size-4" aria-hidden="true" /> Close room
          </Button>
          <template v-else>
            <p class="text-sm">Close the room for everyone? This can't be undone.</p>
            <div class="mt-3 flex gap-2">
              <Button variant="destructive" size="sm" :disabled="closing" @click="emit('close')">
                Close it
              </Button>
              <Button variant="outline" size="sm" @click="confirmingClose = false">Keep it</Button>
            </div>
          </template>
        </template>
        <Button
          v-else-if="mySeat"
          variant="outline"
          class="w-full"
          :disabled="leaving"
          @click="emit('leave')"
        >
          <LogOut class="size-4" aria-hidden="true" /> Leave the table
        </Button>
      </div>
    </aside>
  </div>
</template>
