<script setup lang="ts">
import { computed, ref } from 'vue'
import { UserCircle } from '@lucide/vue'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import ApiKeysManager from '@/components/account/ApiKeysManager.vue'
import SetUsernameDialog from '@/components/collection/SetUsernameDialog.vue'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()

usePageMeta({ title: 'Your profile', canonicalPath: '/profile', noindex: true })

const memberSince = computed(() => {
  const ts = auth.user?.created_at
  if (!ts) return '—'
  const date = new Date(ts)
  if (Number.isNaN(date.getTime())) return '—'
  return date.toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' })
})

// The public handle (issue #362): `username#0001`, set the first time a collection is made
// public. Shown read-only here; changed from a collection page's "make public" flow.
const username = computed(() => auth.user?.username ?? null)
const paddedTag = computed(() => {
  const disc = auth.user?.discriminator
  return disc == null ? '' : `#${String(disc).padStart(4, '0')}`
})

// Set the handle right here when there is none yet (issue #400) — the same "choose a
// username" dialog the collection/deck "make public" flows use; on save it pushes the
// updated user into the auth store, so `username` above re-renders on its own.
const usernameDialogOpen = ref(false)
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
              {{ auth.user?.username ?? 'Collector' }}
            </CardTitle>
            <CardDescription class="truncate">{{ auth.user?.email }}</CardDescription>
          </div>
        </div>
      </CardHeader>
      <CardContent class="grid gap-4 sm:grid-cols-2">
        <div class="flex flex-col items-start gap-1">
          <span class="text-muted-foreground text-xs">Username</span>
          <p v-if="username" class="text-sm">
            {{ username }}<span class="text-muted-foreground">{{ paddedTag }}</span>
          </p>
          <Button v-else variant="outline" size="sm" @click="usernameDialogOpen = true">
            Set username
          </Button>
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

    <div class="mt-6">
      <ApiKeysManager />
    </div>

    <SetUsernameDialog v-model:open="usernameDialogOpen" />
  </div>
</template>
