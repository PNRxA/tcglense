<script setup lang="ts">
import { computed, ref } from 'vue'
import { Loader2 } from '@lucide/vue'
import { RouterLink, useRoute } from 'vue-router'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ApiError, resetPassword } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'

usePageMeta({ title: 'Choose a new password', canonicalPath: '/reset-password', noindex: true })

const route = useRoute()
// The emailed link carries ?token=…; anything else (missing, repeated) is invalid.
const token = computed(() =>
  typeof route.query.token === 'string' && route.query.token ? route.query.token : null,
)

const password = ref('')
const error = ref<string | null>(null)
const loading = ref(false)
const done = ref(false)

async function onSubmit() {
  if (!token.value) return
  error.value = null
  loading.value = true
  try {
    await resetPassword({ token: token.value, password: password.value })
    done.value = true
  } catch (err) {
    error.value = err instanceof ApiError ? err.message : 'Something went wrong. Please try again.'
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="flex min-h-[calc(100vh-3.5rem)] items-center justify-center px-4 py-12">
    <Card class="w-full max-w-sm">
      <template v-if="!token">
        <CardHeader>
          <CardTitle class="text-2xl">Invalid link</CardTitle>
          <CardDescription>
            This password-reset link is incomplete. Request a fresh one and use the newest email.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Button as-child class="w-full">
            <RouterLink to="/forgot-password">Request a new link</RouterLink>
          </Button>
        </CardContent>
      </template>
      <template v-else-if="done">
        <CardHeader>
          <CardTitle class="text-2xl">Password updated</CardTitle>
          <CardDescription>
            Your password has been changed and any existing sessions were signed out. Sign in with
            the new password.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Button as-child class="w-full">
            <RouterLink to="/login">Go to sign in</RouterLink>
          </Button>
        </CardContent>
      </template>
      <template v-else>
        <CardHeader>
          <CardTitle class="text-2xl">Choose a new password</CardTitle>
          <CardDescription
            >This link works once and expires an hour after it was sent.</CardDescription
          >
        </CardHeader>
        <CardContent>
          <form class="flex flex-col gap-4" @submit.prevent="onSubmit">
            <div class="flex flex-col gap-2">
              <Label for="password">New password</Label>
              <Input
                id="password"
                v-model="password"
                type="password"
                autocomplete="new-password"
                minlength="8"
                required
              />
              <p class="text-muted-foreground text-xs">Must be at least 8 characters.</p>
            </div>
            <p v-if="error" class="text-destructive text-sm" role="alert">
              {{ error }}
              <template v-if="error === 'invalid or expired token'">
                <RouterLink to="/forgot-password" class="text-primary font-medium hover:underline">
                  Request a new link
                </RouterLink>
              </template>
            </p>
            <Button type="submit" class="w-full" :disabled="loading">
              <Loader2 v-if="loading" class="animate-spin" />
              {{ loading ? 'Updating password...' : 'Update password' }}
            </Button>
          </form>
        </CardContent>
      </template>
    </Card>
  </div>
</template>
