<script setup lang="ts">
import { computed, toRef } from 'vue'
import { Bell, CalendarDays } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import ReleaseEntry from '@/components/releases/ReleaseEntry.vue'
import { buttonVariants } from '@/components/ui/button'
import { useGameName } from '@/composables/useCatalog'
import { useReleaseCalendarQuery } from '@/composables/useReleases'
import {
  RELEASE_HEADS_UP_PATH,
  calendarMonths,
  calendarWindow,
  isUpcoming,
  releasesPath,
} from '@/lib/releases'
import { usePageMeta } from '@/lib/seo'

// The release calendar (issue #679): the page behind the release heads-ups. Month sections of
// what's coming and what just landed — sets (with the decks and sealed products they ship)
// and Secret Lair drops — each entry linking to the page it already has, with one button to
// the alert settings for anyone who'd rather be told the day before.
//
// A fact page: every date is the catalog's own (`card_sets.released_at`, a drop's cards, a
// product's contents) and the API lists exactly what the heads-ups would notify about, so the
// two can't disagree. Public, like the rest of the catalog — nothing per-user rides the read.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')
const gameName = useGameName(game)

// Today is read once, at setup: the window, the month sections and the upcoming count all key
// on it, and a page that re-read the clock per render could flip an entry from "Releases" to
// "Released" under the reader. The window is month-aligned so the request URL is the same for
// every visitor all month (see `calendarWindow`).
const today = new Date()
const range = calendarWindow(today)
const query = useReleaseCalendarQuery(game, range)

const months = computed(() => (query.data.value ? calendarMonths(query.data.value, today) : []))
const entries = computed(() => months.value.flatMap((month) => month.entries))
const upcomingCount = computed(
  () => entries.value.filter((entry) => isUpcoming(entry, today)).length,
)

usePageMeta({
  title: () => `${gameName.value} release calendar`,
  description: () =>
    `Upcoming and recent ${gameName.value} releases by month — sets, Secret Lair drops, ` +
    `preconstructed decks and sealed products, with release dates from the catalog and a ` +
    `day-before heads-up on TCGLense.`,
  canonicalPath: () => releasesPath(game.value),
})
</script>

<template>
  <div class="mx-auto max-w-4xl px-4 py-10">
    <PageBreadcrumbs :items="[{ label: 'Releases', to: '/releases' }, { label: gameName }]" />

    <header class="mb-8 flex flex-wrap items-start justify-between gap-4">
      <div>
        <h1 class="flex items-center gap-2 text-3xl font-semibold tracking-tight">
          <CalendarDays class="size-7" />
          {{ gameName }} release calendar
        </h1>
        <p class="text-muted-foreground mt-2 max-w-prose">
          What's coming and what just landed, month by month: sets with the decks and sealed
          products they ship, and Secret Lair drops. Dates are the catalog's own.
        </p>
        <p v-if="query.data.value" class="text-muted-foreground mt-1 text-sm">
          {{ entries.length }} {{ entries.length === 1 ? 'release' : 'releases' }}
          <template v-if="upcomingCount"> · {{ upcomingCount }} upcoming</template>
        </p>
      </div>
      <div class="flex flex-col items-start gap-1 sm:items-end">
        <!-- The bridge to the subscription this page is the face of. Signed out, the route
             prompts for sign-in and returns here — the button doesn't second-guess it. -->
        <RouterLink :to="RELEASE_HEADS_UP_PATH" :class="buttonVariants({ variant: 'default' })">
          <Bell />
          Get a heads-up
        </RouterLink>
        <p class="text-muted-foreground text-xs">A notification the day before a release.</p>
      </div>
    </header>

    <LoadingRow v-if="query.isPending.value" label="Loading releases…" />
    <p v-else-if="query.isError.value" class="text-destructive py-12">
      Couldn't load the release calendar. Please retry.
    </p>

    <div v-else class="space-y-10">
      <section v-for="month in months" :key="month.key" :aria-label="month.label">
        <div class="mb-3 flex items-baseline gap-2 border-b pb-2">
          <h2 class="text-xl font-semibold tracking-tight">{{ month.label }}</h2>
          <span v-if="month.current" class="bg-info/15 text-info rounded-md px-1.5 py-0.5 text-xs">
            This month
          </span>
          <span class="text-muted-foreground text-sm">
            {{ month.entries.length }} {{ month.entries.length === 1 ? 'release' : 'releases' }}
          </span>
        </div>
        <p v-if="!month.entries.length" class="text-muted-foreground py-2 text-sm">
          Nothing in the catalog for this month yet.
        </p>
        <div v-else class="space-y-3">
          <ReleaseEntry
            v-for="entry in month.entries"
            :key="entry.key"
            :game="game"
            :entry="entry"
          />
        </div>
      </section>
    </div>
  </div>
</template>
