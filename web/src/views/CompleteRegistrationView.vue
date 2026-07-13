<script setup lang="ts">
import { computed, ref } from 'vue'
import { Loader2 } from '@lucide/vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useTurnstile } from '@/composables/useTurnstile'
import { ApiError, completeRegistration } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'

usePageMeta({
  title: 'Finish creating your account',
  canonicalPath: '/complete-registration',
  noindex: true,
})

const auth = useAuthStore()
const router = useRouter()
const route = useRoute()
// The emailed link carries ?token=…; anything else (missing, repeated) is invalid.
const token = computed(() =>
  typeof route.query.token === 'string' && route.query.token ? route.query.token : null,
)

const username = ref('')
const password = ref('')
const error = ref<string | null>(null)
const loading = ref(false)
const turnstileEl = ref<HTMLElement>()
const { execute } = useTurnstile(turnstileEl)

async function onSubmit() {
  if (!token.value) return
  error.value = null
  loading.value = true
  try {
    const captcha_token = (await execute()) ?? undefined
    const response = await completeRegistration({
      token: token.value,
      password: password.value,
      username: username.value.trim() || null,
      captcha_token,
    })
    // Completion returns a session (access token + refresh cookie) — adopt it and
    // land on the homepage signed in.
    auth.setSession(response.access_token, response.user)
    await router.push('/')
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
            This link is incomplete. Start again and use the newest email we send you.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Button as-child class="w-full">
            <RouterLink to="/register">Start again</RouterLink>
          </Button>
        </CardContent>
      </template>
      <template v-else>
        <CardHeader>
          <CardTitle class="text-2xl">Finish creating your account</CardTitle>
          <CardDescription>Choose a password to activate your account and sign in.</CardDescription>
        </CardHeader>
        <CardContent>
          <form class="flex flex-col gap-4" @submit.prevent="onSubmit">
            <div class="flex flex-col gap-2">
              <Label for="username">Username (optional)</Label>
              <Input
                id="username"
                v-model="username"
                autocomplete="username"
                placeholder="ada_lovelace"
                maxlength="20"
                autocapitalize="off"
                spellcheck="false"
              />
              <p class="text-muted-foreground text-xs">
                Used for your public collection link — you can set or change it later.
              </p>
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
            <p v-if="error" class="text-destructive text-sm" role="alert">
              {{ error }}
              <template v-if="error === 'invalid or expired token'">
                <RouterLink to="/register" class="text-primary font-medium hover:underline">
                  Request a fresh link
                </RouterLink>
                <!-- A dead token usually means the registration was already
                     completed (the link is single-use) — signing in is the fix. -->
                or, if you already finished creating your account,
                <RouterLink to="/login" class="text-primary font-medium hover:underline">
                  sign in
                </RouterLink>
              </template>
            </p>
            <div ref="turnstileEl" class="empty:hidden"></div>
            <Button type="submit" class="w-full" :disabled="loading">
              <Loader2 v-if="loading" class="animate-spin" />
              {{ loading ? 'Creating account...' : 'Create account' }}
            </Button>
          </form>
        </CardContent>
      </template>
    </Card>
  </div>
</template>
