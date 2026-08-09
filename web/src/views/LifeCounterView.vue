<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { HeartPulse, Repeat2, Swords, Zap } from '@lucide/vue'
import { RouterLink, useRouter } from 'vue-router'
import { Button } from '@/components/ui/button'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import StaleNotice from '@/components/cards/StaleNotice.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import LifeSignInPrompt from '@/components/life/LifeSignInPrompt.vue'
import NewGameDialog from '@/components/life/NewGameDialog.vue'
import SessionRow from '@/components/life/SessionRow.vue'
import { useGameName } from '@/composables/useCatalog'
import { useLifeSessionsQuery, useStartLifeSessionMutation } from '@/composables/useLifeCounter'
import type { StartLifeSessionBody } from '@/lib/api/life'
import { lifeDeckStatsPath, lifePath, lifeSessionPath, toolsPath } from '@/lib/tools'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'
import { useLifeSetupStore } from '@/stores/lifeSetup'

// The life counter's landing: resume a game, start one, or look at how your decks are doing.
//
// Ordering is by what you're most likely to want: a game already in progress first (you opened
// this page mid-game to get back to it), then the log of finished ones. The quick-start button
// exists because the overwhelmingly common case is "the same pod, again" — it starts a game from
// the remembered shape without opening the dialog at all.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')

const auth = useAuthStore()
const router = useRouter()
const gameName = useGameName(game)
const setup = useLifeSetupStore()

// `isLoadingError`/`isRefetchError` rather than bare `isError`: query-core keeps cached `data`
// when a background refetch fails, so gating the list on `isError` would swap a loaded list for
// the error paragraph on one transient failure (issue #622).
const { data, isPending, isLoadingError, isRefetchError } = useLifeSessionsQuery(game)
const sessions = computed(() => data.value?.data ?? [])
const active = computed(() => sessions.value.filter((s) => s.status === 'active'))
const finished = computed(() => sessions.value.filter((s) => s.status !== 'active'))

const start = useStartLifeSessionMutation()
const starting = ref(false)

async function startGame(body: StartLifeSessionBody) {
  starting.value = true
  try {
    const detail = await start.mutateAsync({ game: game.value, body })
    await router.push(lifeSessionPath(game.value, detail.session.id))
  } finally {
    starting.value = false
  }
}

/** One tap: the shape you played last time, with unnamed seats the server fills in. */
function quickStart() {
  return startGame({
    starting_life: setup.startingLife,
    layout: setup.layout,
    format: setup.format || undefined,
    // Sent explicitly, like every other remembered field: leaving it out would let the server
    // fall back to the format's default, handing back a counter this pod had turned off.
    counters: [...setup.counters],
    players: Array.from({ length: setup.playerCount }, () => ({})),
  })
}

const quickStartLabel = computed(
  () => `Start ${setup.playerCount}-player · ${setup.startingLife} life`,
)

const crumbs = computed(() => [
  { label: 'Home', to: '/' },
  { label: 'Tools', to: '/tools' },
  { label: gameName.value, to: toolsPath(game.value) },
  { label: 'Life counter' },
])

usePageMeta({
  title: () => `${gameName.value} life counter`,
  description: () =>
    `Track life totals for a table of ${gameName.value} players, keep every gain and loss, ` +
    'and build a win record for your decks.',
  canonicalPath: () => lifePath(game.value),
  // Per-user pages have nothing for a crawler to index.
  noindex: true,
})
</script>

<template>
  <div class="mx-auto max-w-4xl px-4 py-8">
    <PageBreadcrumbs :items="crumbs" />

    <LifeSignInPrompt v-if="auth.sessionResolved && !auth.isAuthenticated" :game-name="gameName" />

    <template v-else>
      <header class="mb-6 flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 class="flex items-center gap-2 text-3xl font-semibold tracking-tight">
            <HeartPulse class="size-7" aria-hidden="true" />
            Life counter
          </h1>
          <p class="text-muted-foreground mt-2">
            Count life at the table. Every change is kept, so you can undo a mis-tap and see how the
            game went.
          </p>
        </div>
        <div class="flex flex-wrap gap-2">
          <Button variant="outline" :disabled="starting" @click="quickStart">
            <Zap class="size-4" /> {{ quickStartLabel }}
          </Button>
          <NewGameDialog :game="game" :busy="starting" @start="startGame" />
        </div>
      </header>

      <p v-if="start.error.value" class="text-destructive mb-4 text-sm">
        {{ start.error.value.message }}
      </p>

      <StaleNotice
        v-if="isRefetchError"
        label="Couldn't refresh — showing your last loaded games."
      />

      <LoadingRow v-if="isPending" label="Loading your games…" />
      <p v-else-if="isLoadingError" class="text-destructive py-12">
        Couldn't load your games. Please retry.
      </p>

      <template v-else>
        <section v-if="active.length" class="mb-8">
          <h2 class="mb-3 text-sm font-medium">In progress</h2>
          <div class="space-y-2">
            <SessionRow v-for="s in active" :key="s.id" :session="s" :game="game" />
          </div>
        </section>

        <section>
          <div class="mb-3 flex items-center justify-between">
            <h2 class="text-sm font-medium">Played</h2>
            <RouterLink
              :to="lifeDeckStatsPath(game)"
              class="text-muted-foreground hover:text-foreground flex items-center gap-1.5 text-sm hover:underline"
            >
              <Swords class="size-4" aria-hidden="true" /> Deck records
            </RouterLink>
          </div>
          <div v-if="finished.length" class="space-y-2">
            <SessionRow v-for="s in finished" :key="s.id" :session="s" :game="game" />
          </div>
          <div v-else-if="!active.length" class="bg-card rounded-xl border p-8 text-center">
            <div class="bg-muted mx-auto grid size-12 place-items-center rounded-lg">
              <Repeat2 class="size-6" aria-hidden="true" />
            </div>
            <p class="mt-4 font-medium">No games tracked yet</p>
            <p class="text-muted-foreground mx-auto mt-2 max-w-sm text-sm">
              Start a game and tap to count life. Link a player to one of your decks and finishing
              the game will start building that deck's win record.
            </p>
          </div>
          <p v-else class="text-muted-foreground text-sm">
            Nothing finished yet — record a result to start your play log.
          </p>
        </section>
      </template>
    </template>
  </div>
</template>
