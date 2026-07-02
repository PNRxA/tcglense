<script setup lang="ts">
import { ref } from 'vue'
import { Loader2, MailCheck } from '@lucide/vue'
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
import { useTurnstile } from '@/composables/useTurnstile'
import { ApiError, register, resendVerification } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'

usePageMeta({ title: 'Create your account', canonicalPath: '/register', noindex: true })

const email = ref('')
const password = ref('')
const displayName = ref('')
const turnstileEl = ref<HTMLElement>()
const { execute } = useTurnstile(turnstileEl)

const error = ref<string | null>(null)
const loading = ref(false)
// Registration mints no session — on success we stay here and show the
// check-your-email step (the canonicalised address the server mailed).
const registeredEmail = ref<string | null>(null)

async function onSubmit() {
  error.value = null
  loading.value = true
  try {
    const captcha_token = (await execute()) ?? undefined
    const response = await register({
      email: email.value,
      password: password.value,
      display_name: displayName.value.trim() || null,
      captcha_token,
    })
    registeredEmail.value = response.user.email
  } catch (err) {
    error.value = err instanceof ApiError ? err.message : 'Something went wrong. Please try again.'
  } finally {
    loading.value = false
  }
}

const resendLoading = ref(false)
const resendSent = ref(false)

async function onResend() {
  if (!registeredEmail.value) return
  resendLoading.value = true
  try {
    const captcha_token = (await execute()) ?? undefined
    await resendVerification({ email: registeredEmail.value, captcha_token })
    resendSent.value = true
  } catch {
    // Generic endpoint; a failure here is transient — the button stays available.
  } finally {
    resendLoading.value = false
  }
}
</script>

<template>
  <div class="flex min-h-[calc(100vh-3.5rem)] items-center justify-center px-4 py-12">
    <div class="w-full max-w-sm">
      <Card v-if="registeredEmail" class="w-full">
        <CardHeader>
          <MailCheck class="text-primary size-8" />
          <CardTitle class="text-2xl">Check your email</CardTitle>
          <CardDescription>
            We sent a verification link to <span class="font-medium">{{ registeredEmail }}</span
            >. Open it to activate your account, then sign in.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Button as-child class="w-full">
            <RouterLink to="/login">Go to sign in</RouterLink>
          </Button>
        </CardContent>
        <CardFooter class="justify-center">
          <p class="text-muted-foreground text-sm">
            <template v-if="resendSent">Verification email sent — check your inbox.</template>
            <template v-else>
              Didn't get it?
              <button
                type="button"
                class="text-primary font-medium hover:underline"
                :disabled="resendLoading"
                @click="onResend"
              >
                Resend the link
              </button>
            </template>
          </p>
        </CardFooter>
      </Card>
      <Card v-else class="w-full">
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
      <!-- Persistent CAPTCHA mount (survives the form -> check-your-email swap so a
         resend can still get a token); invisible unless a challenge is needed. -->
      <div ref="turnstileEl" class="mt-4 flex justify-center empty:hidden"></div>
    </div>
  </div>
</template>
