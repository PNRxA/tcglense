<script setup lang="ts">
import { computed, toRef } from 'vue'
import { RouterLink } from 'vue-router'
import { Layers, Library, UserCircle } from '@lucide/vue'
import { usePublicProfileQuery } from '@/composables/usePublicCollection'
import { usePublicDecksQuery } from '@/composables/useDecks'
import { formatUsd } from '@/lib/money'
import { usePageMeta } from '@/lib/seo'

// A user's public profile (issues #361/#362): their handle + the game collections and decks
// (issue #391) they've made public, each linking to its read-only page. Unauthenticated and
// indexable. A 404 (unknown handle or nothing public) renders the not-found state.
const props = defineProps<{ handle: string }>()
const handle = toRef(props, 'handle')

const profileQuery = usePublicProfileQuery(handle)
const profile = computed(() => profileQuery.data.value)
const notFound = computed(() => profileQuery.isError.value)

// Public decks are fetched separately (a cross-game list, its own endpoint). Its 404 —
// "this user has no public deck" — is expected and stays SILENT: the section only renders
// when there are decks, and the page's not-found state is driven by the profile query
// alone, never by this one.
const decksQuery = usePublicDecksQuery(handle)
const publicDecks = computed(() => decksQuery.data.value?.data ?? [])

const displayName = computed(() => profile.value?.username || '')
const tag = computed(() =>
  profile.value ? `#${String(profile.value.discriminator).padStart(4, '0')}` : '',
)

usePageMeta({
  title: () => (displayName.value ? `${displayName.value}'s collections` : 'Collection'),
  description: () =>
    displayName.value ? `${displayName.value}'s public trading-card collections on TCGLense.` : '',
  canonicalPath: () => `/u/${handle.value}`,
})
</script>

<template>
  <div class="mx-auto max-w-4xl px-4 py-10">
    <!-- Unknown handle / no public games. -->
    <div v-if="notFound" class="py-20 text-center">
      <h1 class="text-2xl font-semibold tracking-tight">Collection not found</h1>
      <p class="text-muted-foreground mt-2">
        This profile doesn't exist or has no public collections.
      </p>
      <RouterLink to="/" class="text-primary mt-4 inline-block underline underline-offset-2">
        Go home
      </RouterLink>
    </div>

    <div v-else-if="profileQuery.isPending.value" class="text-muted-foreground py-20 text-center">
      Loading…
    </div>

    <template v-else-if="profile">
      <header class="mb-8 flex items-center gap-4">
        <div class="bg-muted flex size-16 shrink-0 items-center justify-center rounded-full">
          <UserCircle class="text-muted-foreground size-9" />
        </div>
        <div class="min-w-0">
          <h1 class="truncate text-2xl font-semibold tracking-tight">
            {{ displayName }}
          </h1>
          <p class="text-muted-foreground font-mono text-sm">
            {{ profile.username }}<span class="opacity-70">{{ tag }}</span>
          </p>
        </div>
      </header>

      <template v-if="profile.games.length">
        <h2 class="text-muted-foreground mb-3 text-xs font-medium tracking-wide uppercase">
          Public collections
        </h2>
        <ul class="grid gap-4 sm:grid-cols-2">
          <li v-for="entry in profile.games" :key="entry.game">
            <RouterLink
              :to="`/u/${handle}/${entry.game}`"
              class="hover:border-primary/60 hover:bg-muted/40 block rounded-xl border p-4 transition-colors"
            >
              <div class="flex items-center gap-2">
                <Library class="text-muted-foreground size-4" />
                <span class="font-medium uppercase">{{ entry.game }}</span>
              </div>
              <dl class="mt-3 flex flex-wrap gap-x-6 gap-y-1 text-sm">
                <div>
                  <dt class="text-muted-foreground text-xs">Unique cards</dt>
                  <dd class="font-semibold tabular-nums">
                    {{ entry.summary.unique_cards.toLocaleString() }}
                  </dd>
                </div>
                <div>
                  <dt class="text-muted-foreground text-xs">Total copies</dt>
                  <dd class="font-semibold tabular-nums">
                    {{ entry.summary.total_cards.toLocaleString() }}
                  </dd>
                </div>
                <div v-if="formatUsd(entry.summary.total_value_usd)">
                  <dt class="text-muted-foreground text-xs">Value</dt>
                  <dd class="font-semibold tabular-nums">
                    {{ formatUsd(entry.summary.total_value_usd) }}
                  </dd>
                </div>
              </dl>
            </RouterLink>
          </li>
        </ul>
      </template>

      <!-- Public decks (issue #391): a flat, cross-game list — each links to the read-only
        public deck view. Only shown when there are any; a decks-less profile omits it. -->
      <template v-if="publicDecks.length">
        <h2 class="text-muted-foreground mt-8 mb-3 text-xs font-medium tracking-wide uppercase">
          Public decks
        </h2>
        <ul class="grid gap-4 sm:grid-cols-2">
          <li v-for="deck in publicDecks" :key="deck.id">
            <RouterLink
              :to="`/u/${handle}/decks/${deck.id}`"
              class="hover:border-primary/60 hover:bg-muted/40 block rounded-xl border p-4 transition-colors"
            >
              <div class="flex items-center gap-2">
                <Layers class="text-muted-foreground size-4 shrink-0" />
                <span class="truncate font-medium" :title="deck.name">{{ deck.name }}</span>
                <span class="text-muted-foreground text-xs uppercase">{{ deck.game }}</span>
              </div>
              <p class="text-muted-foreground mt-1 text-sm">
                {{ deck.card_count }} card{{ deck.card_count === 1 ? '' : 's' }}
                <span v-if="deck.format"> · {{ deck.format }}</span>
              </p>
            </RouterLink>
          </li>
        </ul>
      </template>
    </template>
  </div>
</template>
