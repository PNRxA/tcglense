<script setup lang="ts">
import { computed, defineAsyncComponent, ref, toRef } from 'vue'
import { useRouter } from 'vue-router'
import { Button } from '@/components/ui/button'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import PlayClosedNotice from '@/components/play/PlayClosedNotice.vue'
import PlayJoinCard from '@/components/play/PlayJoinCard.vue'
import PlayLobby from '@/components/play/PlayLobby.vue'
import { useGameName } from '@/composables/useCatalog'
import { usePlayRoomSession } from '@/composables/usePlayRoomSession'
import { useDeletePlayRoom, useLeavePlaySeat } from '@/composables/usePlayRooms'
import { playPath, playRoomPath, toolsPath } from '@/lib/tools'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'
import { usePlayRoomStore } from '@/stores/playRoom'

// One room: the lobby until the host starts, then the table.
//
// The page has two shapes and switches wholesale. In the lobby it's an ordinary page with
// breadcrumbs and chrome; once the game is running the table takes the viewport with the life
// counter's focus idiom (`fixed inset-0 z-50`), because a virtual battlefield competing with a
// site header for vertical space is a worse battlefield. `PlayTable` reads the store directly
// and takes no props, so nothing about the game state passes through here.
//
// It is loaded async for a second reason beyond weight: the table is a large surface that lands
// separately from this page's plumbing, and an async import means the lobby type-checks and
// renders whether or not that chunk is built yet.
const props = defineProps<{ game: string; code: string }>()
const game = toRef(props, 'game')
const code = computed(() => props.code.toUpperCase())

const auth = useAuthStore()
const router = useRouter()
const gameName = useGameName(game)
const store = usePlayRoomStore()

const PlayTable = defineAsyncComponent(() => import('@/components/play/table/PlayTable.vue'))

const session = usePlayRoomSession(game, code)

const leaveSeat = useLeavePlaySeat()
const deleteRoom = useDeletePlayRoom()
const starting = ref(false)

/** The host's Start is a socket frame, not a REST call: the server builds the table in place. */
function start() {
  starting.value = true
  // A refused start answers with an `error` frame the store surfaces; nothing to await.
  store.start()
  window.setTimeout(() => {
    starting.value = false
  }, 2000)
}

async function leave() {
  const seatId = session.seatId.value
  if (seatId === null) return
  await leaveSeat.mutateAsync({
    game: game.value,
    code: code.value,
    seatId,
    seatToken: session.seatToken.value ?? undefined,
  })
  session.clearSeat()
  await router.push(playPath(game.value))
}

/**
 * Leaving the *table* is walking away from the screen, not giving up the seat: the token
 * stays in storage and the room stays in the hub's list, so the invite link (or that list)
 * brings them straight back to the same chair. Only the lobby's Leave, above, gets up.
 */
async function leaveTable() {
  store.reset()
  await router.push(playPath(game.value))
}

async function closeRoom() {
  await deleteRoom.mutateAsync({ game: game.value, code: code.value })
  session.clearSeat()
  await router.push(playPath(game.value))
}

async function removeSeat(seatId: number) {
  await leaveSeat.mutateAsync({ game: game.value, code: code.value, seatId })
}

const crumbs = computed(() => [
  { label: 'Tools', to: '/tools' },
  { label: gameName.value, to: toolsPath(game.value) },
  { label: 'Play online', to: playPath(game.value) },
  { label: session.room.value?.label || code.value },
])

/** The lobby is the whole page until the socket hands us a table. */
const atTable = computed(() => store.hasTable)

usePageMeta({
  title: () => `${session.room.value?.label || code.value} — play ${gameName.value} online`,
  canonicalPath: () => playRoomPath(game.value, code.value),
  noindex: true,
})
</script>

<template>
  <!-- The table owns the viewport; every scrap of page chrome is gone while it's up. -->
  <div v-if="atTable" class="bg-background fixed inset-0 z-50 flex flex-col">
    <PlayTable @leave="leaveTable" />
    <!-- ...except the one message that must survive it: the room closing under the players. -->
    <PlayClosedNotice
      v-if="session.closedReason.value"
      overlay
      :reason="session.closedReason.value"
      :back-to="playPath(game)"
    />
  </div>

  <div v-else class="mx-auto max-w-5xl px-4 py-8">
    <PageBreadcrumbs :items="crumbs" />

    <!-- A room the server closed under us: say so, and offer the only useful next step. -->
    <PlayClosedNotice
      v-if="session.closedReason.value"
      :reason="session.closedReason.value"
      :back-to="playPath(game)"
    />

    <div v-else-if="session.notFound.value" class="bg-card rounded-xl border p-8 text-center">
      <p class="font-medium">No such room</p>
      <p class="text-muted-foreground mx-auto mt-2 max-w-sm text-sm">
        The code <span class="font-mono">{{ code }}</span> doesn't match a room. Codes are six
        characters — check it with whoever sent it, or open a table of your own.
      </p>
      <Button class="mt-4" variant="outline" @click="router.push(playPath(game))">
        Back to Play online
      </Button>
    </div>

    <LoadingRow v-else-if="session.isLoading.value" label="Loading the room…" />

    <!-- We hold a seat we couldn't claim *this second*. The token is still ours, so the only
      thing on offer is trying again — joining afresh would take a second seat. -->
    <div
      v-else-if="session.phase.value === 'retry'"
      class="bg-card rounded-xl border p-8 text-center"
    >
      <p class="font-medium">Couldn't reach your seat</p>
      <p class="text-muted-foreground mx-auto mt-2 max-w-sm text-sm">
        {{ session.error.value || 'The table is not answering right now.' }} Your seat is still
        yours — try again in a moment.
      </p>
      <Button class="mt-4" :disabled="session.isJoining.value" @click="session.retry()">
        Try again
      </Button>
    </div>

    <!-- Still working out which seat is ours: not a lobby yet, and definitely not a spectator. -->
    <LoadingRow v-else-if="session.phase.value === 'resolving'" label="Finding your seat…" />

    <template v-else-if="session.room.value">
      <PlayJoinCard
        v-if="session.phase.value === 'needs-name' || session.phase.value === 'blocked'"
        :room="session.room.value"
        :is-authenticated="auth.isAuthenticated"
        :busy="session.isJoining.value"
        :error="session.error.value"
        @join="session.join"
        @watch="session.watch"
      />

      <PlayLobby
        v-else
        :game="game"
        :code="code"
        :room="session.room.value"
        :my-seat="session.seat.value"
        :seat-token="session.seatToken.value"
        :starting="starting"
        :leaving="leaveSeat.isPending.value"
        :closing="deleteRoom.isPending.value"
        @start="start"
        @leave="leave"
        @close="closeRoom"
        @remove="removeSeat"
      />

      <p v-if="store.lastError" class="text-destructive mt-4 text-sm">
        {{ store.lastError.message }}
      </p>
      <p v-if="leaveSeat.error.value" class="text-destructive mt-4 text-sm">
        {{ leaveSeat.error.value.message }}
      </p>
      <p v-if="deleteRoom.error.value" class="text-destructive mt-4 text-sm">
        {{ deleteRoom.error.value.message }}
      </p>
    </template>

    <LoadingRow v-else label="Finding your seat…" />
  </div>
</template>
