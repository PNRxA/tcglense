<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { ArrowRight, ClipboardList, Link2, Swords, Users } from '@lucide/vue'
import { useRouter } from 'vue-router'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import StaleNotice from '@/components/cards/StaleNotice.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import PlayCreateRoomForm from '@/components/play/PlayCreateRoomForm.vue'
import PlayRoomRow from '@/components/play/PlayRoomRow.vue'
import PlaySignInPrompt from '@/components/play/PlaySignInPrompt.vue'
import { useGameName } from '@/composables/useCatalog'
import { useCreatePlayRoom, useDeletePlayRoom, usePlayRoomsQuery } from '@/composables/usePlayRooms'
import type { CreatePlayRoomBody, PlayRoomSummary } from '@/lib/api/play'
import { PLAY_CODE_LENGTH } from '@/lib/api/play'
import { rememberSeatToken } from '@/lib/playSeat'
import { playRoomPath, playPath, toolsPath } from '@/lib/tools'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'

// The play table's landing: get back to a game, open a new one, or walk in with a code.
//
// The three-way split is deliberate and survives being signed out: creating a room and listing
// yours need an account (a room has an owner), but *joining by code* is the guest path and must
// keep working with no session at all — so the sign-in prompt replaces only the left column and
// the code box sits beside it either way. Without that, an invite code read out over voice chat
// would dead-end at a login wall, which is the one thing this feature can't afford.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')

const auth = useAuthStore()
const router = useRouter()
const gameName = useGameName(game)

const rooms = usePlayRoomsQuery(game)
const all = computed(() => rooms.data.value?.data ?? [])
/** In-progress first (the row you came back for), then lobbies, then the log of finished ones. */
const ORDER = { playing: 0, lobby: 1, finished: 2 } as const
const ordered = computed(() => [...all.value].sort((a, b) => ORDER[a.status] - ORDER[b.status]))

const create = useCreatePlayRoom()
const remove = useDeletePlayRoom()

async function createRoom(body: CreatePlayRoomBody) {
  const result = await create.mutateAsync({ game: game.value, body })
  // The host's own seat token, remembered before we navigate: the room page replays it instead
  // of re-joining, so a refresh on the way in can't cost the host their seat.
  rememberSeatToken(game.value, result.room.code, result.seat_token)
  await router.push(playRoomPath(game.value, result.room.code))
}

function deleteRoom(code: string) {
  return remove.mutateAsync({ game: game.value, code })
}

/** Whether to offer the host's close action on a row: the caller's own seat is the host's. */
function hosts(room: PlayRoomSummary): boolean {
  return room.seats.some((seat) => seat.is_host && seat.id === room.viewer_seat)
}

// ---- Join by code ----
const code = ref('')
const normalisedCode = computed(() => code.value.trim().toUpperCase())
const codeReady = computed(() => normalisedCode.value.length === PLAY_CODE_LENGTH)

function openCode() {
  if (!codeReady.value) return
  return router.push(playRoomPath(game.value, normalisedCode.value))
}

const crumbs = computed(() => [
  { label: 'Home', to: '/' },
  { label: 'Tools', to: '/tools' },
  { label: gameName.value, to: toolsPath(game.value) },
  { label: 'Play online' },
])

usePageMeta({
  title: () => `Play ${gameName.value} online`,
  description: () =>
    `Open a table, share the link, and play a manual game of ${gameName.value} with friends — ` +
    'your decks, any precon, or a pasted list.',
  canonicalPath: () => playPath(game.value),
  // Rooms are private to whoever holds the link; there's nothing here to index.
  noindex: true,
})
</script>

<template>
  <div class="mx-auto max-w-5xl px-4 py-8">
    <PageBreadcrumbs :items="crumbs" />

    <header class="mb-8">
      <h1 class="flex items-center gap-2 text-3xl font-semibold tracking-tight">
        <Swords class="size-7" aria-hidden="true" />
        Play online
      </h1>
      <p class="text-muted-foreground mt-2 max-w-2xl">
        Open a table, send your friends the link, and play a manual game of {{ gameName }} in the
        browser. Nothing is enforced — it's paper, with the shuffling done for you.
      </p>
      <ul class="text-muted-foreground mt-4 grid gap-3 text-sm sm:grid-cols-3">
        <li class="flex items-start gap-2">
          <Link2 class="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          One invite link; guests join with just a name.
        </li>
        <li class="flex items-start gap-2">
          <ClipboardList class="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          Play one of your decks, any precon, or a pasted list.
        </li>
        <li class="flex items-start gap-2">
          <Users class="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          Two to six players around a shared table.
        </li>
      </ul>
    </header>

    <div class="grid gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]">
      <div class="space-y-6">
        <PlaySignInPrompt
          v-if="auth.sessionResolved && !auth.isAuthenticated"
          :game-name="gameName"
        />

        <template v-else>
          <PlayCreateRoomForm
            :busy="create.isPending.value"
            :error="create.error.value?.message ?? null"
            @create="createRoom"
          />

          <section>
            <h2 class="mb-3 text-sm font-medium">Your rooms</h2>

            <StaleNotice
              v-if="rooms.isRefetchError.value"
              label="Couldn't refresh — showing your last loaded rooms."
            />

            <LoadingRow v-if="rooms.isPending.value" label="Loading your rooms…" />
            <p v-else-if="rooms.isLoadingError.value" class="text-destructive py-8">
              Couldn't load your rooms. Please retry.
            </p>
            <div v-else-if="ordered.length" class="space-y-2">
              <PlayRoomRow
                v-for="room in ordered"
                :key="room.id"
                :room="room"
                :game="game"
                :can-delete="hosts(room)"
                :deleting="remove.isPending.value"
                @delete="deleteRoom"
              />
            </div>
            <p
              v-else
              class="text-muted-foreground bg-card rounded-xl border p-8 text-center text-sm"
            >
              No rooms yet. Create one above and send the link to your table.
            </p>

            <p v-if="remove.error.value" class="text-destructive mt-3 text-sm">
              {{ remove.error.value.message }}
            </p>
          </section>
        </template>
      </div>

      <!-- Always available, signed in or not: this is the guest's way in. -->
      <aside>
        <form class="bg-card space-y-3 rounded-xl border p-4" @submit.prevent="openCode">
          <div>
            <h2 class="font-medium">Join with a code</h2>
            <p class="text-muted-foreground mt-1 text-sm">
              Got {{ PLAY_CODE_LENGTH }} characters from a friend? No account needed.
            </p>
          </div>
          <Label for="play-join-code" class="sr-only">Room code</Label>
          <Input
            id="play-join-code"
            v-model="code"
            :maxlength="PLAY_CODE_LENGTH"
            autocapitalize="characters"
            autocomplete="off"
            spellcheck="false"
            placeholder="ABC234"
            class="text-center font-mono text-lg tracking-[0.3em] uppercase"
          />
          <Button type="submit" class="w-full" :disabled="!codeReady">
            Join <ArrowRight class="size-4" aria-hidden="true" />
          </Button>
        </form>
      </aside>
    </div>
  </div>
</template>
