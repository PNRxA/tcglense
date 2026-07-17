<script setup lang="ts">
import { computed, ref } from 'vue'
import { Loader2 } from '@lucide/vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useSecretToken } from '@/composables/useSecretToken'
import { useTurnstile } from '@/composables/useTurnstile'
import { ApiError } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'
import { safeInternalPath } from '@/lib/utils'
import { useAuthStore } from '@/stores/auth'

usePageMeta({
  title: 'Finish creating your account',
  canonicalPath: '/complete-registration',
  noindex: true,
})

const auth = useAuthStore()
const router = useRouter()
const route = useRoute()
const token = useSecretToken()
const redirect = computed(() => safeInternalPath(route.query.redirect))
const registerTo = computed(() => ({
  path: '/register',
  ...(redirect.value ? { query: { redirect: redirect.value } } : {}),
}))
const loginTo = computed(() => ({
  path: '/login',
  ...(redirect.value ? { query: { redirect: redirect.value } } : {}),
}))

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
    await auth.completeRegistration({
      token: token.value,
      password: password.value,
      username: username.value.trim() || null,
      captcha_token,
    })
    // The store adopts the returned session while ordering its refresh-cookie write
    // after any older refresh. Return to the feature that prompted signup.
    await router.push(redirect.value ?? '/')
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
            <RouterLink :to="registerTo">Start again</RouterLink>
          </Button>
        </CardContent>
      </template>
      <template v-else>
        <CardHeader>
          <CardTitle class="text-2xl">Finish creating your account</CardTitle>
          <CardDescription>Choose a password to activate your account and sign in.</CardDescription>
        </CardHeader>
        <CardContent>
          <form
            class="flex flex-col gap-4"
            :aria-busy="loading || undefined"
            @submit.prevent="onSubmit"
          >
            <div class="flex flex-col gap-2">
              <Label for="username">Username (optional)</Label>
              <Input
                id="username"
                v-model="username"
                name="username"
                autocomplete="username"
                placeholder="ada_lovelace"
                minlength="3"
                maxlength="20"
                pattern="[A-Za-z0-9_]+"
                autocapitalize="off"
                spellcheck="false"
                :aria-invalid="Boolean(error) || undefined"
                :aria-describedby="
                  error ? 'complete-username-help complete-error' : 'complete-username-help'
                "
              />
              <p id="complete-username-help" class="text-muted-foreground text-xs">
                3–20 letters, numbers, or underscores. Used for public links; you can set it later.
              </p>
            </div>
            <div class="flex flex-col gap-2">
              <Label for="password">Password</Label>
              <Input
                id="password"
                v-model="password"
                name="password"
                type="password"
                autocomplete="new-password"
                minlength="8"
                maxlength="1024"
                required
                :aria-invalid="Boolean(error) || undefined"
                :aria-describedby="
                  error ? 'complete-password-help complete-error' : 'complete-password-help'
                "
              />
              <p id="complete-password-help" class="text-muted-foreground text-xs">
                Must be at least 8 characters.
              </p>
            </div>
            <p v-if="error" id="complete-error" class="text-destructive text-sm" role="alert">
              {{ error }}
              <template v-if="error === 'invalid or expired token'">
                <RouterLink :to="registerTo" class="text-primary font-medium hover:underline">
                  Request a fresh link
                </RouterLink>
                <!-- A dead token usually means the registration was already
                     completed (the link is single-use) — signing in is the fix. -->
                or, if you already finished creating your account,
                <RouterLink :to="loginTo" class="text-primary font-medium hover:underline">
                  sign in
                </RouterLink>
              </template>
            </p>
            <div ref="turnstileEl" class="empty:hidden"></div>
            <Button type="submit" class="w-full" :disabled="loading">
              <Loader2 v-if="loading" class="animate-spin" />
              {{ loading ? 'Creating account...' : 'Create account' }}
            </Button>
            <p class="text-muted-foreground text-center text-xs text-pretty">
              By creating an account, you agree to the
              <RouterLink
                to="/terms"
                target="_blank"
                rel="noopener noreferrer"
                class="text-primary underline-offset-4 hover:underline"
              >
                Terms of Service
              </RouterLink>
              and
              <RouterLink
                to="/privacy"
                target="_blank"
                rel="noopener noreferrer"
                class="text-primary underline-offset-4 hover:underline"
              >
                Privacy Policy</RouterLink
              >.
            </p>
          </form>
        </CardContent>
      </template>
    </Card>
  </div>
</template>
