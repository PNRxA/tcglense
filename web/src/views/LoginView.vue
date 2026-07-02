<script setup lang="ts">
import { ref } from 'vue'
import { Loader2 } from '@lucide/vue'
import { RouterLink } from 'vue-router'
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
import { useAuthSubmit } from '@/composables/useAuthSubmit'
import { useTurnstile } from '@/composables/useTurnstile'
import { resendVerification } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const { error, errorStatus, loading, submit } = useAuthSubmit()

usePageMeta({ title: 'Sign in', canonicalPath: '/login', noindex: true })

const email = ref('')
const password = ref('')
const turnstileEl = ref<HTMLElement>()
const { execute } = useTurnstile(turnstileEl)

function onSubmit() {
  submit(async () => {
    const captcha_token = (await execute()) ?? undefined
    await auth.login({ email: email.value, password: password.value, captcha_token })
  })
}

// A 403 means the credentials were right but the email is unverified — offer to
// resend the verification link (the server answers generically either way).
const resendLoading = ref(false)
const resendSent = ref(false)

async function onResend() {
  resendLoading.value = true
  try {
    const captcha_token = (await execute()) ?? undefined
    await resendVerification({ email: email.value, captcha_token })
    resendSent.value = true
  } catch {
    // Generic endpoint; a failure here is transient — leave the prompt in place.
  } finally {
    resendLoading.value = false
  }
}
</script>

<template>
  <div class="flex min-h-[calc(100vh-3.5rem)] items-center justify-center px-4 py-12">
    <Card class="w-full max-w-sm">
      <CardHeader>
        <CardTitle class="text-2xl">Welcome back</CardTitle>
        <CardDescription>Sign in to your TCGLense account</CardDescription>
      </CardHeader>
      <CardContent>
        <form class="flex flex-col gap-4" @submit.prevent="onSubmit">
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
            <div class="flex items-center justify-between">
              <Label for="password">Password</Label>
              <RouterLink
                to="/forgot-password"
                class="text-muted-foreground text-xs hover:underline"
              >
                Forgot password?
              </RouterLink>
            </div>
            <Input
              id="password"
              v-model="password"
              type="password"
              autocomplete="current-password"
              required
            />
          </div>
          <p v-if="error" class="text-destructive text-sm" role="alert">{{ error }}</p>
          <p v-if="errorStatus === 403" class="text-muted-foreground text-sm">
            <template v-if="resendSent">
              Verification email sent — check your inbox, then sign in.
            </template>
            <template v-else>
              Check your inbox for the verification link, or
              <button
                type="button"
                class="text-primary font-medium hover:underline"
                :disabled="resendLoading"
                @click="onResend"
              >
                resend it</button
              >.
            </template>
          </p>
          <div ref="turnstileEl" class="empty:hidden"></div>
          <Button type="submit" class="w-full" :disabled="loading">
            <Loader2 v-if="loading" class="animate-spin" />
            {{ loading ? 'Signing in...' : 'Sign in' }}
          </Button>
        </form>
      </CardContent>
      <CardFooter class="justify-center">
        <p class="text-muted-foreground text-sm">
          Don't have an account?
          <RouterLink to="/register" class="text-primary font-medium hover:underline">
            Create one
          </RouterLink>
        </p>
      </CardFooter>
    </Card>
  </div>
</template>
