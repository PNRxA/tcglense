<script setup lang="ts">
import { computed, type Component } from 'vue'
import {
  ArrowRight,
  ChevronRight,
  CloudDownload,
  Eye,
  EyeOff,
  Ghost,
  LibraryBig,
  Sparkles,
  TrendingUp,
  Wallet,
} from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { useGamesQuery } from '@/composables/useCatalog'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()

usePageMeta({
  description:
    'Browse trading-card games, sets, and cards, follow singles prices over time, and track ' +
    'your collection — with ghost mode showing exactly which cards you are missing.',
  canonicalPath: '/',
})

// Optional, resilient to an empty/loading list: lets a visitor jump straight into a
// real game's public catalog from the homepage.
const gamesQuery = useGamesQuery()
const games = computed(() => gamesQuery.data.value?.data ?? [])

interface Feature {
  icon: Component
  title: string
  description: string
  to: string
}

// Only real, shipped features — each links to where it actually lives. Descriptions
// carry concrete, verifiable specifics (price windows, the reconcile-mode names) so the
// claims stay honest and checkable.
const features: Feature[] = [
  {
    icon: LibraryBig,
    title: 'Card catalog',
    description: 'Browse games, sets, and cards, with Scryfall-style search on every list.',
    to: '/cards',
  },
  {
    icon: TrendingUp,
    title: 'Singles price history',
    description:
      'Daily USD, EUR, and foil prices charted on every card — windowed from the last 7 days ' +
      'to the full history.',
    to: '/cards',
  },
  {
    icon: Wallet,
    title: 'Your collection',
    description: 'Track regular and foil copies per game, with a live value and count summary.',
    to: '/collection',
  },
  {
    icon: CloudDownload,
    title: 'Import and sync',
    description:
      'Pull in from Archidekt by link (overwrite, replace, merge, or smart) or CSV, then ' +
      're-sync a saved link on demand.',
    to: '/collection',
  },
  {
    icon: Sparkles,
    title: 'Owned-count badges',
    description: 'Signed in, every card you already own is badged as you browse the catalog.',
    to: '/cards',
  },
]

// The ghost-mode selling points, kept truthful: it is a toggle inside a collection grid.
const ghostPoints = [
  'A “Show ghosts” toggle on any collection card grid',
  'A live “X of Y owned” count as you scan a set',
  'Quick-add a missing card right where it sits',
]
</script>

<template>
  <div class="mx-auto max-w-5xl px-4 py-16 sm:py-24">
    <!-- Hero: conversion-first value prop + primary/secondary CTAs, auth-branched. -->
    <section class="flex flex-col items-center text-center">
      <span
        class="border-border bg-muted text-muted-foreground mb-6 inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs font-medium"
      >
        <Ghost class="size-3.5" aria-hidden="true" />
        New — collection ghost mode
      </span>
      <h1 class="max-w-2xl text-4xl font-semibold tracking-tight text-balance sm:text-5xl">
        Track every card. Watch every price.
      </h1>
      <p class="text-muted-foreground mt-4 max-w-xl text-base text-pretty sm:text-lg">
        Browse games, sets, and cards, follow real singles prices as they move, and track your
        collection down to the last foil — then flip on ghost mode to see exactly which cards you
        are missing.
      </p>

      <div class="mt-8 flex flex-col items-center gap-3 sm:flex-row">
        <template v-if="auth.isAuthenticated">
          <RouterLink to="/collection" :class="buttonVariants({ size: 'lg' })">
            Open your collection
            <ArrowRight aria-hidden="true" />
          </RouterLink>
          <RouterLink to="/cards" :class="buttonVariants({ variant: 'outline', size: 'lg' })">
            Browse cards
          </RouterLink>
        </template>
        <template v-else>
          <RouterLink to="/register" :class="buttonVariants({ size: 'lg' })">
            Create your account
            <ArrowRight aria-hidden="true" />
          </RouterLink>
          <RouterLink to="/cards" :class="buttonVariants({ variant: 'outline', size: 'lg' })">
            Browse cards — no account needed
          </RouterLink>
        </template>
      </div>

      <p v-if="!auth.isAuthenticated" class="text-muted-foreground mt-4 text-sm text-pretty">
        Just looking?
        <RouterLink to="/cards" class="text-primary underline-offset-4 hover:underline">
          Browse the catalog
        </RouterLink>
        — no account needed. Already have an account?
        <RouterLink to="/login" class="text-primary underline-offset-4 hover:underline">
          Sign in
        </RouterLink>
      </p>
    </section>

    <!-- Ghost mode spotlight: the headline new feature (issue #112), placed first. -->
    <section class="mt-16 sm:mt-20">
      <h2 class="sr-only">Collection ghost mode</h2>
      <Card class="border-primary/30 bg-primary/5 gap-0 overflow-hidden rounded-3xl py-0">
        <div class="grid gap-8 p-6 sm:p-10 md:grid-cols-2 md:items-center">
          <div>
            <span
              class="border-primary/30 bg-primary/10 text-primary inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs font-medium"
            >
              <Ghost class="size-3.5" aria-hidden="true" />
              New — ghost mode
            </span>
            <p class="mt-4 text-2xl font-semibold tracking-tight text-balance sm:text-3xl">
              See the gaps in your collection
            </p>
            <p class="text-muted-foreground mt-3 text-pretty">
              Flip the <span class="text-foreground font-medium">Show ghosts</span> toggle on any
              collection grid and the cards you do not own appear dimmed as ghosts beside the ones
              you do — so a set’s holes read at a glance, and you can quick-add a missing card right
              in place.
            </p>
            <ul class="mt-5 space-y-2.5">
              <li v-for="point in ghostPoints" :key="point" class="flex items-start gap-2.5">
                <ChevronRight class="text-primary mt-0.5 size-4 shrink-0" aria-hidden="true" />
                <span class="text-muted-foreground text-sm text-pretty">{{ point }}</span>
              </li>
            </ul>
            <div class="mt-6">
              <RouterLink to="/collection" :class="buttonVariants({ size: 'lg' })">
                <Ghost aria-hidden="true" />
                {{ auth.isAuthenticated ? 'Open your collection' : 'Try ghost mode' }}
              </RouterLink>
              <p v-if="!auth.isAuthenticated" class="text-muted-foreground mt-2 text-sm">
                Sign-in required — it is free to create an account.
              </p>
            </div>
          </div>

          <!-- Visual demo: one owned card beside two dimmed "ghosts". Decorative. -->
          <div
            class="bg-background/60 flex flex-col items-center gap-3 rounded-2xl border p-6"
            aria-hidden="true"
          >
            <div class="flex items-end justify-center gap-3 sm:gap-4">
              <div class="flex flex-col items-center gap-2">
                <div
                  class="bg-card ring-primary/30 flex size-20 items-center justify-center rounded-xl border shadow-sm ring-1"
                >
                  <Eye class="text-primary size-9" />
                </div>
                <span class="text-foreground text-xs font-medium">Owned</span>
              </div>
              <div class="flex flex-col items-center gap-2 opacity-40">
                <div
                  class="flex size-20 items-center justify-center rounded-xl border border-dashed"
                >
                  <Ghost class="text-muted-foreground size-9" />
                </div>
                <span class="text-muted-foreground text-xs font-medium">Ghost</span>
              </div>
              <div class="flex flex-col items-center gap-2 opacity-40">
                <div
                  class="flex size-20 items-center justify-center rounded-xl border border-dashed"
                >
                  <EyeOff class="text-muted-foreground size-9" />
                </div>
                <span class="text-muted-foreground text-xs font-medium">Ghost</span>
              </div>
            </div>
            <p class="text-muted-foreground text-xs">1 of 3 owned</p>
          </div>
        </div>
      </Card>
    </section>

    <!-- Feature grid: the real, live features, each a link to where it lives. -->
    <section class="mt-16">
      <h2 class="text-xl font-semibold tracking-tight">Everything you can do today</h2>
      <p class="text-muted-foreground mt-1 text-sm">Every feature here is live right now.</p>
      <div class="mt-6 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        <RouterLink
          v-for="feature in features"
          :key="feature.title"
          :to="feature.to"
          class="bg-card hover:border-ring/60 hover:bg-accent/40 group flex items-center gap-4 rounded-xl border p-5 transition-colors"
        >
          <span class="bg-muted flex size-12 shrink-0 items-center justify-center rounded-lg">
            <component :is="feature.icon" class="text-primary size-6" aria-hidden="true" />
          </span>
          <span class="min-w-0">
            <span class="text-foreground block font-medium">{{ feature.title }}</span>
            <span class="text-muted-foreground mt-0.5 block text-sm text-pretty">
              {{ feature.description }}
            </span>
          </span>
          <ChevronRight
            class="text-muted-foreground ml-auto size-5 shrink-0 transition-transform group-hover:translate-x-0.5"
            aria-hidden="true"
          />
        </RouterLink>
      </div>
    </section>

    <!-- Games strip: jump straight into a real game's catalog (public, no account). -->
    <section v-if="games.length" class="mt-16">
      <h2 class="text-xl font-semibold tracking-tight">Start with your game</h2>
      <p class="text-muted-foreground mt-1 text-sm">Browse the full catalog, no account needed.</p>
      <div class="mt-4 flex flex-wrap gap-2">
        <RouterLink
          v-for="game in games"
          :key="game.id"
          :to="`/cards/${game.id}`"
          class="bg-card hover:border-ring/60 hover:bg-accent/40 inline-flex items-center gap-2 rounded-full border px-4 py-2 text-sm font-medium transition-colors"
        >
          <LibraryBig class="text-muted-foreground size-4" aria-hidden="true" />
          {{ game.name }}
          <ChevronRight class="text-muted-foreground size-4" aria-hidden="true" />
        </RouterLink>
      </div>
    </section>

    <!-- Closing CTA band: repeat the primary conversion ask, auth-branched. -->
    <section class="mt-16 sm:mt-20">
      <div class="bg-card rounded-2xl border p-8 text-center sm:p-12">
        <h2 class="text-2xl font-semibold tracking-tight text-balance sm:text-3xl">
          {{ auth.isAuthenticated ? 'Pick up where you left off' : 'Start tracking in minutes' }}
        </h2>
        <p
          v-if="auth.isAuthenticated"
          class="text-muted-foreground mx-auto mt-3 max-w-xl text-pretty"
        >
          Jump back into your collection and see what you are still chasing.
        </p>
        <p v-else class="text-muted-foreground mx-auto mt-3 max-w-xl text-pretty">
          Create a free account to start tracking your collection, or keep browsing the catalog — no
          sign-up needed.
        </p>
        <div class="mt-6 flex flex-col items-center justify-center gap-3 sm:flex-row">
          <template v-if="auth.isAuthenticated">
            <RouterLink to="/collection" :class="buttonVariants({ size: 'lg' })">
              Open your collection
              <ArrowRight aria-hidden="true" />
            </RouterLink>
            <RouterLink to="/cards" :class="buttonVariants({ variant: 'outline', size: 'lg' })">
              Browse cards
            </RouterLink>
          </template>
          <template v-else>
            <RouterLink to="/register" :class="buttonVariants({ size: 'lg' })">
              Create your account
              <ArrowRight aria-hidden="true" />
            </RouterLink>
            <RouterLink to="/login" :class="buttonVariants({ variant: 'outline', size: 'lg' })">
              Sign in
            </RouterLink>
          </template>
        </div>
      </div>
    </section>

    <!-- Honest roadmap footnote: unbuilt work is clearly future-tense. -->
    <p class="text-muted-foreground mt-12 text-center text-sm text-pretty">
      Singles pricing is live today. Sealed-product pricing and full set-completion tracking are on
      the way.
    </p>
  </div>
</template>
