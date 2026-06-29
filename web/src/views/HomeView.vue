<script setup lang="ts">
import { Layers, LibraryBig, Sparkles, TrendingUp } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()

const features = [
  {
    icon: TrendingUp,
    title: 'Price history',
    description:
      'Follow retail, MSRP, and singles prices over time so you buy and sell at the right moment.',
  },
  {
    icon: LibraryBig,
    title: 'Your collection',
    description: 'Catalogue everything you own across games and keep its value at your fingertips.',
  },
  {
    icon: Layers,
    title: 'Set completion',
    description: 'See exactly how close you are to finishing every set you are chasing.',
  },
]
</script>

<template>
  <div class="mx-auto max-w-5xl px-4 py-16 sm:py-24">
    <section class="flex flex-col items-center text-center">
      <span
        class="border-border bg-muted text-muted-foreground mb-6 inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs font-medium"
      >
        <Sparkles class="size-3.5" />
        Early preview
      </span>
      <h1 class="max-w-2xl text-4xl font-semibold tracking-tight text-balance sm:text-5xl">
        Track every card. Watch every price.
      </h1>
      <p class="text-muted-foreground mt-4 max-w-xl text-base text-pretty sm:text-lg">
        TCGLense is your home for trading-card prices, collection tracking, and set-completion
        progress. We are just getting started — check back soon.
      </p>

      <div class="mt-8 flex flex-col items-center gap-3 sm:flex-row">
        <RouterLink
          v-if="auth.isAuthenticated"
          to="/dashboard"
          :class="buttonVariants({ size: 'lg' })"
        >
          Go to your dashboard
        </RouterLink>
        <template v-else>
          <RouterLink to="/register" :class="buttonVariants({ size: 'lg' })">
            Create your account
          </RouterLink>
          <RouterLink to="/login" :class="buttonVariants({ variant: 'outline', size: 'lg' })">
            Sign in
          </RouterLink>
        </template>
      </div>
    </section>

    <section class="mt-16 grid gap-4 sm:grid-cols-3">
      <h2 class="sr-only">Features</h2>
      <Card v-for="feature in features" :key="feature.title">
        <CardHeader>
          <CardTitle class="flex items-center gap-2 text-base">
            <component :is="feature.icon" class="text-muted-foreground size-5" />
            {{ feature.title }}
          </CardTitle>
          <CardDescription>{{ feature.description }}</CardDescription>
        </CardHeader>
      </Card>
    </section>
  </div>
</template>
