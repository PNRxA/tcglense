<script setup lang="ts">
import { computed } from 'vue'
import { UserCircle } from '@lucide/vue'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()

const memberSince = computed(() => {
  const ts = auth.user?.created_at
  if (!ts) return '—'
  const date = new Date(ts)
  if (Number.isNaN(date.getTime())) return '—'
  return date.toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' })
})
</script>

<template>
  <div class="mx-auto max-w-2xl px-4 py-12">
    <div class="mb-8">
      <h1 class="text-3xl font-semibold tracking-tight">Profile</h1>
      <p class="text-muted-foreground mt-2">Manage your TCGLense account.</p>
    </div>

    <Card>
      <CardHeader>
        <div class="flex items-center gap-4">
          <div
            class="bg-muted text-muted-foreground flex size-16 shrink-0 items-center justify-center rounded-full"
          >
            <UserCircle class="size-10" />
          </div>
          <div class="min-w-0">
            <CardTitle class="truncate text-xl">
              {{ auth.user?.display_name ?? 'Collector' }}
            </CardTitle>
            <CardDescription class="truncate">{{ auth.user?.email }}</CardDescription>
          </div>
        </div>
      </CardHeader>
      <CardContent class="grid gap-4 sm:grid-cols-2">
        <div class="flex flex-col gap-1">
          <span class="text-muted-foreground text-xs">Display name</span>
          <p class="text-sm">{{ auth.user?.display_name ?? 'Not set' }}</p>
        </div>
        <div class="flex flex-col gap-1">
          <span class="text-muted-foreground text-xs">Email</span>
          <p class="text-sm">{{ auth.user?.email ?? '—' }}</p>
        </div>
        <div class="flex flex-col gap-1">
          <span class="text-muted-foreground text-xs">Member since</span>
          <p class="text-sm">{{ memberSince }}</p>
        </div>
      </CardContent>
    </Card>

    <p class="text-muted-foreground mt-6 text-center text-sm">
      Profile editing and collection stats are coming soon.
    </p>
  </div>
</template>
