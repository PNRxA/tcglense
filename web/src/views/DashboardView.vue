<script setup lang="ts">
import { Sparkles } from '@lucide/vue'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()

usePageMeta({ title: 'Dashboard', canonicalPath: '/dashboard', noindex: true })
</script>

<template>
  <div class="mx-auto max-w-5xl px-4 py-12">
    <div class="mb-8">
      <h1 class="text-3xl font-semibold tracking-tight">
        Welcome, {{ auth.user?.display_name ?? auth.user?.email ?? 'collector' }}
      </h1>
      <p class="text-muted-foreground mt-2">Your TCGLense dashboard is taking shape.</p>
    </div>

    <Card>
      <CardHeader>
        <CardTitle class="flex items-center gap-2">
          <Sparkles class="size-5" />
          Coming soon
        </CardTitle>
        <CardDescription>What you'll be able to do with TCGLense</CardDescription>
      </CardHeader>
      <CardContent>
        <ul class="text-muted-foreground list-disc space-y-2 pl-5 text-sm">
          <li>Track your trading card collection across multiple games.</li>
          <li>Monitor card values and price history over time.</li>
          <li>Organize cards into decks, binders, and wishlists.</li>
          <li>Scan cards with your camera to add them instantly.</li>
        </ul>
      </CardContent>
    </Card>
  </div>
</template>
