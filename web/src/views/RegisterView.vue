<script setup lang="ts">
import { ref } from 'vue'
import { Loader2, MailCheck } from '@lucide/vue'
import { RouterLink, useRouter } from 'vue-router'
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
import { ApiError, register } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'

usePageMeta({ title: 'Create your account', canonicalPath: '/register', noindex: true })

const router = useRouter()

const email = ref('')
const turnstileEl = ref<HTMLElement>()
const { execute } = useTurnstile(turnstileEl)

const error = ref<string | null>(null)
const loading = ref(false)
// Registration is email-first: on success we normally stay here and show the
// check-your-email step (the address we mailed a completion link to). The one
// exception is the no-email dev bypass, where the response carries the completion
// token directly and we jump straight to the set-password step (see below).
const registeredEmail = ref<string | null>(null)

async function onSubmit() {
  error.value = null
  loading.value = true
  try {
    const captcha_token = (await execute()) ?? undefined
    const response = await register({ email: email.value, captcha_token })
    if (response.completion_token) {
      // No-email dev bypass: the completion link couldn't be emailed, so its token
      // rides in the response — drive straight to the set-password step.
      await router.push({
        path: '/complete-registration',
        query: { token: response.completion_token },
      })
    } else {
      // The server no longer echoes the address, so canonicalise the submitted one
      // the same way it does (trim + lowercase) for the confirmation copy.
      registeredEmail.value = email.value.trim().toLowerCase()
    }
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
    // Re-registering the same address re-sends the completion link (generic 200,
    // 60s cooldown) — the same call the form makes.
    await register({ email: registeredEmail.value, captcha_token })
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
            We sent a link to <span class="font-medium">{{ registeredEmail }}</span> to finish
            creating your account. Open it to choose a password and sign in.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Button as-child class="w-full">
            <RouterLink to="/login">Go to sign in</RouterLink>
          </Button>
        </CardContent>
        <CardFooter class="justify-center">
          <p class="text-muted-foreground text-sm">
            <template v-if="resendSent">Link sent — check your inbox.</template>
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
              <Label for="email">Email</Label>
              <Input
                id="email"
                v-model="email"
                type="email"
                autocomplete="email"
                placeholder="you@example.com"
                required
              />
              <p class="text-muted-foreground text-xs">
                We'll email you a link to finish creating your account.
              </p>
            </div>
            <p v-if="error" class="text-destructive text-sm" role="alert">{{ error }}</p>
            <Button type="submit" class="w-full" :disabled="loading">
              <Loader2 v-if="loading" class="animate-spin" />
              {{ loading ? 'Sending link...' : 'Continue' }}
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
