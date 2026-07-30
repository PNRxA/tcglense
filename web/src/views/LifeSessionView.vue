<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, toRef } from 'vue'
import { History, Maximize2, Minimize2, Repeat2, Trash2, Undo2, UserPlus } from '@lucide/vue'
import { useRouter } from 'vue-router'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import FinishGameDialog from '@/components/life/FinishGameDialog.vue'
import LifeHistoryPanel from '@/components/life/LifeHistoryPanel.vue'
import LifeMat from '@/components/life/LifeMat.vue'
import LifeSignInPrompt from '@/components/life/LifeSignInPrompt.vue'
import SeatSettingsDialog from '@/components/life/SeatSettingsDialog.vue'
import SessionScoreboard from '@/components/life/SessionScoreboard.vue'
import WakeLockPill from '@/components/life/WakeLockPill.vue'
import { useGameName } from '@/composables/useCatalog'
import {
  useAddLifePlayerMutation,
  useDeleteLifeSessionMutation,
  useRemoveLifePlayerMutation,
  useReorderLifePlayersMutation,
  useStartLifeSessionMutation,
  useUpdateLifePlayerMutation,
} from '@/composables/useLifeCounter'
import { useLifeSession } from '@/composables/useLifeSession'
import { durationLabel } from '@/lib/lifeSeries'
import { lifePath, lifeSessionPath, toolsPath } from '@/lib/tools'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'
import type { LifeRotation } from '@/lib/api/life'

// The live counter: the table between the players.
//
// The mat gets the whole viewport below a slim toolbar, because during a game everything else is
// noise — the history sits below it (scroll for it) rather than beside it, and focus mode drops
// even the toolbar's page chrome. The reactive engine is `useLifeSession`; this view is the
// chrome around it plus the four structural mutations (add/edit/remove a seat, rematch) that
// aren't part of the tap loop.
const props = defineProps<{ game: string; sessionId: string }>()
const game = toRef(props, 'game')
const sessionId = computed(() => Number(props.sessionId))

const auth = useAuthStore()
const router = useRouter()
const gameName = useGameName(game)

const life = useLifeSession(game, sessionId)
const {
  query,
  session,
  seats,
  events,
  isActive,
  lines,
  seatViews,
  grid,
  lastEvent,
  focused,
  elapsed,
  wakeLock,
  taps,
} = life

onMounted(life.startTicker)
onBeforeUnmount(life.stopTicker)

const addPlayer = useAddLifePlayerMutation()
const updatePlayer = useUpdateLifePlayerMutation()
const removePlayer = useRemoveLifePlayerMutation()
const reorderPlayers = useReorderLifePlayersMutation()
const deleteSession = useDeleteLifeSessionMutation()
const rematch = useStartLifeSessionMutation()

const finishOpen = ref(false)
const settingsFor = ref<number | null>(null)
const settingsSeat = computed(
  () => seats.value.find((seat) => seat.id === settingsFor.value) ?? null,
)

const winnerId = computed(() => seats.value.find((seat) => seat.result === 'win')?.id ?? null)

const title = computed(
  () => session.value?.name || `${seats.value.length}-player ${session.value?.format ?? 'game'}`,
)

const crumbs = computed(() => [
  { label: 'Tools', to: '/tools' },
  { label: gameName.value, to: toolsPath(game.value) },
  { label: 'Life counter', to: lifePath(game.value) },
  { label: title.value },
])

usePageMeta({
  title: () => `${title.value} — ${gameName.value} life counter`,
  canonicalPath: () => lifeSessionPath(game.value, sessionId.value),
  noindex: true,
})

async function saveSeat(value: {
  name: string
  deck_id: number | null
  commander_card_id: string | null
  rotation: LifeRotation
}) {
  if (settingsFor.value === null) return
  await updatePlayer.mutateAsync({
    game: game.value,
    sessionId: sessionId.value,
    playerId: settingsFor.value,
    body: value,
  })
}

async function correctLife(value: number) {
  if (settingsFor.value === null) return
  await life.setLife(settingsFor.value, value)
}

async function dropSeat() {
  if (settingsFor.value === null) return
  await removePlayer.mutateAsync({
    game: game.value,
    sessionId: sessionId.value,
    playerId: settingsFor.value,
  })
}

/**
 * Shift one seat a place earlier or later in the layout's seat order — the other half of
 * "where does everyone sit" (Facing turns a tile; this swaps which spot of the layout it holds).
 * Sent as the whole permutation, which is what the endpoint requires.
 */
async function moveSeat(direction: -1 | 1) {
  const from = seats.value.findIndex((seat) => seat.id === settingsFor.value)
  const to = from + direction
  if (from < 0 || to < 0 || to >= seats.value.length) return
  const ids = seats.value.map((seat) => seat.id)
  const moved = ids[from]
  const displaced = ids[to]
  if (moved === undefined || displaced === undefined) return
  ids[from] = displaced
  ids[to] = moved
  await reorderPlayers.mutateAsync({
    game: game.value,
    sessionId: sessionId.value,
    playerIds: ids,
  })
}

async function seatAnother() {
  await addPlayer.mutateAsync({ game: game.value, sessionId: sessionId.value, body: {} })
}

/** Same table, fresh totals — the server copies the seats and their decks. */
async function playAgain() {
  const detail = await rematch.mutateAsync({
    game: game.value,
    body: { from_session_id: sessionId.value },
  })
  await router.push(lifeSessionPath(game.value, detail.session.id))
}

async function abandon() {
  await deleteSession.mutateAsync({ game: game.value, sessionId: sessionId.value })
  await router.push(lifePath(game.value))
}

async function undoLast() {
  if (lastEvent.value) await life.undoEvent(lastEvent.value.id)
}

const finishedDuration = computed(() =>
  session.value?.finished_at ? durationLabel(elapsed.value) : null,
)
</script>

<template>
  <LifeSignInPrompt v-if="!auth.isAuthenticated" :game-name="gameName" />

  <div v-else-if="query.isPending.value" class="mx-auto max-w-4xl px-4 py-12">
    <LoadingRow label="Loading game…" />
  </div>

  <div v-else-if="query.isError.value" class="mx-auto max-w-4xl px-4 py-12">
    <p class="text-destructive">
      {{
        query.error.value?.status === 404 ? "That game doesn't exist." : "Couldn't load the game."
      }}
    </p>
    <Button class="mt-4" variant="outline" @click="router.push(lifePath(game))">
      Back to the life counter
    </Button>
  </div>

  <div
    v-else-if="session"
    :class="
      focused
        ? 'bg-background fixed inset-0 z-50 flex flex-col'
        : 'mx-auto flex max-w-6xl flex-col px-4 py-6'
    "
  >
    <PageBreadcrumbs v-if="!focused" :items="crumbs" />

    <!-- Toolbar: the game's identity and the handful of actions that aren't tapping. -->
    <div class="flex flex-wrap items-center gap-2 pb-3" :class="focused ? 'px-3 pt-3' : ''">
      <h1 class="min-w-0 truncate text-lg font-semibold">{{ title }}</h1>
      <span class="text-muted-foreground shrink-0 text-sm tabular-nums">
        {{ finishedDuration ?? durationLabel(elapsed) }}
      </span>
      <WakeLockPill
        v-if="isActive"
        :supported="wakeLock.supported"
        :active="wakeLock.active.value"
      />
      <span
        v-if="!isActive"
        class="bg-muted text-muted-foreground shrink-0 rounded-full px-2 py-0.5 text-xs font-medium"
      >
        Finished
      </span>

      <div class="ml-auto flex shrink-0 items-center gap-2">
        <Button
          v-if="isActive"
          variant="ghost"
          size="icon-sm"
          :disabled="!lastEvent || life.isUndoing.value"
          aria-label="Undo the last life change"
          title="Undo the last life change"
          @click="undoLast"
        >
          <Undo2 class="size-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          :aria-label="focused ? 'Exit focus mode' : 'Focus mode'"
          :title="focused ? 'Exit focus mode' : 'Focus mode — the table fills the screen'"
          @click="focused = !focused"
        >
          <Minimize2 v-if="focused" class="size-4" />
          <Maximize2 v-else class="size-4" />
        </Button>
        <Button v-if="isActive" size="sm" @click="finishOpen = true">Finish game</Button>
        <Button
          v-else
          size="sm"
          variant="outline"
          :disabled="rematch.isPending.value"
          @click="playAgain"
        >
          <Repeat2 class="size-4" /> Play again
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <Button variant="ghost" size="icon-sm" aria-label="Game options">
              <History class="size-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem v-if="isActive" @select="seatAnother">
              <UserPlus class="size-4" /> Add a player
            </DropdownMenuItem>
            <DropdownMenuItem v-if="!isActive" @select="playAgain">
              <Repeat2 class="size-4" /> Play again
            </DropdownMenuItem>
            <DropdownMenuItem class="text-destructive" @select="abandon">
              <Trash2 class="size-4" /> Delete this game
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>

    <p v-if="taps.error.value" class="text-destructive pb-2 text-sm" role="alert">
      Couldn't save that change ({{ taps.error.value.message }}) — the total shown is the last one
      saved.
    </p>

    <!-- The table. Sized to the viewport so the whole pod is visible without scrolling; the
         history lives below it. -->
    <div class="min-h-0" :class="focused ? 'flex-1 px-3 pb-3' : 'h-[min(70dvh,44rem)]'">
      <LifeMat
        v-if="isActive"
        :seats="seatViews"
        :grid="grid"
        :editable="true"
        :winner-id="winnerId"
        :game-slug="game"
        @bump="life.bump"
        @settings="(id) => (settingsFor = id)"
      />
      <div v-else class="h-full overflow-y-auto">
        <SessionScoreboard :seats="seats" :lines="lines" :game="game" />
      </div>
    </div>

    <div v-if="!focused" class="pt-6">
      <LifeHistoryPanel
        :lines="lines"
        :events="events"
        :seats="seats"
        :started-at="session.started_at"
        :undoable="isActive"
        :busy="life.isUndoing.value"
        @undo="life.undoEvent"
      />
    </div>

    <FinishGameDialog
      v-model:open="finishOpen"
      :seats="seats"
      :busy="life.isFinishing.value"
      @finish="
        (winner) => {
          finishOpen = false
          life.finishGame(winner)
        }
      "
    />

    <SeatSettingsDialog
      :open="settingsFor !== null"
      :seat="settingsSeat"
      :game="game"
      :removable="seats.length > 1"
      :seat-count="seats.length"
      :busy="
        updatePlayer.isPending.value ||
        removePlayer.isPending.value ||
        reorderPlayers.isPending.value
      "
      @update:open="(open) => !open && (settingsFor = null)"
      @save="saveSeat"
      @set-life="correctLife"
      @move="moveSeat"
      @remove="dropSeat"
    />
  </div>
</template>
