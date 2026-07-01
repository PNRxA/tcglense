<script setup lang="ts">
import { ref } from 'vue'
import { Loader2 } from '@lucide/vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ApiError } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'
import { safeInternalPath } from '@/lib/utils'
import { useAuthStore } from '@/stores/auth'

const router = useRouter()
const route = useRoute()
const auth = useAuthStore()

usePageMeta({ title: 'Create your account', canonicalPath: '/register', noindex: true })

const email = ref('')
const password = ref('')
const displayName = ref('')
const error = ref<string | null>(null)
const loading = ref(false)

async function onSubmit() {
  error.value = null
  loading.value = true
  try {
    await auth.register({
      email: email.value,
      password: password.value,
      display_name: displayName.value.trim() || null,
    })
    await router.push(safeInternalPath(route.query.redirect) ?? '/dashboard')
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
      <CardHeader>
        <CardTitle class="text-2xl">Create your account</CardTitle>
        <CardDescription>Start tracking your collection with TCGLense</CardDescription>
      </CardHeader>
      <CardContent>
        <form class="flex flex-col gap-4" @submit.prevent="onSubmit">
          <div class="flex flex-col gap-2">
            <Label for="display-name">Display name (optional)</Label>
            <Input
              id="display-name"
              v-model="displayName"
              autocomplete="nickname"
              placeholder="Ash"
            />
          </div>
          <div class="flex flex-col gap-2">
            <Label for="email">Email</Label>
            <Input
              id="email"
              v-model="email"
              type="email"
              autocomplete="email"
              placeholder="you@example.com"
              required
            />
          </div>
          <div class="flex flex-col gap-2">
            <Label for="password">Password</Label>
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
          <p v-if="error" class="text-destructive text-sm" role="alert">{{ error }}</p>
          <Button type="submit" class="w-full" :disabled="loading">
            <Loader2 v-if="loading" class="animate-spin" />
            {{ loading ? 'Creating account...' : 'Create account' }}
          </Button>
        </form>
      </CardContent>
      <CardFooter class="justify-center">
        <p class="text-muted-foreground text-sm">
          Already have an account?
          <RouterLink to="/login" class="text-primary font-medium hover:underline">
            Sign in
          </RouterLink>
        </p>
      </CardFooter>
    </Card>
  </div>
</template>
