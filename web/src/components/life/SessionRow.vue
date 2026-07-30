<script setup lang="ts">
import { computed } from 'vue'
import { ChevronRight, Crown, Handshake, Play } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import type { LifeSession } from '@/lib/api'
import { durationLabel, seatColor, sessionDuration } from '@/lib/lifeSeries'
import { lifeSessionPath } from '@/lib/tools'

// One tracked game in the list: who played, what they played, how it ended.
//
// An in-progress game is the row you're most likely to want, so it says so and leads with a
// "Resume" affordance rather than reading like the finished ones above it.
const props = defineProps<{ session: LifeSession; game: string }>()

const isActive = computed(() => props.session.status === 'active')
const winner = computed(() => props.session.players.find((seat) => seat.result === 'win'))
const draw = computed(
  () =>
    props.session.status === 'finished' &&
    !winner.value &&
    props.session.players.some((seat) => seat.result === 'draw'),
)
const duration = computed(() =>
  sessionDuration(props.session.started_at, props.session.finished_at),
)
const startedLabel = computed(() =>
  new Date(props.session.started_at).toLocaleString(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }),
)
const title = computed(
  () =>
    props.session.name ||
    `${props.session.players.length}-player ${props.session.format ?? 'game'}`,
)
</script>

<template>
  <RouterLink
    :to="lifeSessionPath(game, session.id)"
    class="bg-card hover:border-ring/60 hover:bg-accent/40 group flex items-center gap-4 rounded-xl border p-4 transition-colors"
    :class="isActive ? 'border-primary/50' : ''"
  >
    <div class="bg-muted grid size-10 shrink-0 place-items-center rounded-lg">
      <Play v-if="isActive" class="size-5" aria-hidden="true" />
      <Crown
        v-else-if="winner"
        class="size-5 text-emerald-600 dark:text-emerald-400"
        aria-hidden="true"
      />
      <Handshake v-else-if="draw" class="size-5" aria-hidden="true" />
      <ChevronRight v-else class="size-5" aria-hidden="true" />
    </div>

    <div class="min-w-0 flex-1">
      <p class="flex items-center gap-2 font-medium">
        <span class="truncate">{{ title }}</span>
        <span
          v-if="isActive"
          class="bg-primary/15 text-primary shrink-0 rounded-full px-2 py-0.5 text-xs font-medium"
        >
          In progress
        </span>
      </p>
      <!-- Seats with their colour dot, so a row reads the same way the mat does. -->
      <p class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-sm">
        <span
          v-for="seat in session.players"
          :key="seat.id"
          class="flex items-center gap-1.5"
          :class="seat.result === 'win' ? 'font-medium' : 'text-muted-foreground'"
        >
          <span
            class="size-1.5 rounded-full"
            :style="{ backgroundColor: seatColor(seat.position) }"
            aria-hidden="true"
          />
          {{ seat.name }}
          <span
            v-if="seat.deck_name ?? seat.commander_name"
            class="text-muted-foreground/80 truncate"
            >· {{ seat.deck_name ?? seat.commander_name }}</span
          >
        </span>
      </p>
      <p class="text-muted-foreground mt-1 text-xs">
        {{ startedLabel }}
        <template v-if="duration !== null"> · {{ durationLabel(duration) }}</template>
        <template v-if="winner"> · {{ winner.name }} won</template>
        <template v-else-if="draw"> · drawn</template>
      </p>
    </div>

    <ChevronRight
      class="text-muted-foreground size-5 shrink-0 transition-transform group-hover:translate-x-0.5"
      aria-hidden="true"
    />
  </RouterLink>
</template>
