<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Loader2, MailCheck } from '@lucide/vue'
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
import { useTurnstile } from '@/composables/useTurnstile'
import { ApiError, register } from '@/lib/api'
import { publicConfig } from '@/lib/config'
import { usePageMeta } from '@/lib/seo'
import { safeInternalPath } from '@/lib/utils'

usePageMeta({ title: 'Create your account', canonicalPath: '/register', noindex: true })

const router = useRouter()
const route = useRoute()
const redirect = computed(() => safeInternalPath(route.query.redirect))
const loginTo = computed(() => ({
  path: '/login',
  ...(redirect.value ? { query: { redirect: redirect.value } } : {}),
}))

const email = ref('')
const turnstileEl = ref<HTMLElement>()
const { execute } = useTurnstile(turnstileEl)

const error = ref<string | null>(null)
const loading = ref(false)

// Whether new signups are open. The API is the source of truth (it 403s a
// disabled register), but reading it up front lets us show the operator's notice
// and disable the form instead of failing on submit. On a config-fetch error we
// leave the form open — a genuinely disabled instance still refuses server-side.
const signupsEnabled = ref(true)
const signupsDisabledMessage = ref<string | null>(null)

onMounted(async () => {
  try {
    const config = await publicConfig()
    signupsEnabled.value = config.signups_enabled
    signupsDisabledMessage.value = config.signups_disabled_message
  } catch {
    // Leave signups enabled; the submit is still guarded by the server.
  }
})
// Registration is email-first: on success we normally stay here and show the
// check-your-email step (the address we mailed a completion link to). The one
// exception is the no-email dev bypass, where the response carries the completion
// token directly and we jump straight to the set-password step (see below).
const registeredEmail = ref<string | null>(null)

async function onSubmit() {
  if (!signupsEnabled.value) return
  error.value = null
  loading.value = true
  try {
    const captcha_token = (await execute()) ?? undefined
    const response = await register({
      email: email.value,
      redirect: redirect.value ?? undefined,
      captcha_token,
    })
    if (response.completion_token) {
      // No-email dev bypass: the completion link couldn't be emailed, so its token
      // rides in the response — drive straight to the set-password step.
      await router.push({
        path: '/complete-registration',
        query: {
          token: response.completion_token,
          ...(redirect.value ? { redirect: redirect.value } : {}),
        },
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
const resendError = ref<string | null>(null)

async function onResend() {
  if (!registeredEmail.value) return
  resendSent.value = false
  resendError.value = null
  resendLoading.value = true
  try {
    const captcha_token = (await execute()) ?? undefined
    // Re-registering the same address re-sends the completion link (generic 200,
    // 60s cooldown) — the same call the form makes.
    await register({
      email: registeredEmail.value,
      redirect: redirect.value ?? undefined,
      captcha_token,
    })
    resendSent.value = true
  } catch (err) {
    resendError.value =
      err instanceof ApiError ? err.message : 'Something went wrong. Please try again.'
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
            If this address can be used to create an account, a link will arrive at
            <span class="font-medium">{{ registeredEmail }}</span
            >. Open it to choose a password and sign in.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Button as-child class="w-full">
            <RouterLink :to="loginTo">Go to sign in</RouterLink>
          </Button>
        </CardContent>
        <CardFooter class="flex-col gap-2 text-center">
          <p v-if="resendSent" class="text-muted-foreground text-sm" role="status">
            If the address is eligible, another link will arrive soon.
          </p>
          <p v-if="resendError" class="text-destructive text-sm" role="alert">
            {{ resendError }}
          </p>
          <p class="text-muted-foreground text-sm">
            Didn't get it?
            <button
              type="button"
              class="text-primary font-medium hover:underline disabled:opacity-50"
              :disabled="resendLoading"
              @click="onResend"
            >
              {{ resendLoading ? 'Requesting...' : 'Request another link' }}
            </button>
          </p>
        </CardFooter>
      </Card>
      <Card v-else class="w-full">
        <CardHeader>
          <CardTitle class="text-2xl">Create your account</CardTitle>
          <CardDescription>Start tracking your collection with TCGLense</CardDescription>
        </CardHeader>
        <CardContent>
          <div
            v-if="!signupsEnabled"
            class="border-border bg-muted text-foreground mb-4 rounded-md border p-3 text-sm"
            role="status"
          >
            {{ signupsDisabledMessage ?? 'New sign-ups are temporarily disabled.' }}
          </div>
          <form
            class="flex flex-col gap-4"
            :aria-busy="loading || undefined"
            @submit.prevent="onSubmit"
          >
            <div class="flex flex-col gap-2">
              <Label for="email">Email</Label>
              <Input
                id="email"
                v-model="email"
                name="email"
                type="email"
                autocomplete="email"
                maxlength="254"
                placeholder="you@example.com"
                required
                :aria-invalid="Boolean(error) || undefined"
                :aria-describedby="
                  error ? 'register-email-help register-error' : 'register-email-help'
                "
                :disabled="!signupsEnabled"
              />
              <p id="register-email-help" class="text-muted-foreground text-xs">
                We'll email you a link to finish creating your account.
              </p>
            </div>
            <p v-if="error" id="register-error" class="text-destructive text-sm" role="alert">
              {{ error }}
            </p>
            <Button type="submit" class="w-full" :disabled="loading || !signupsEnabled">
              <Loader2 v-if="loading" class="animate-spin" />
              {{ loading ? 'Sending link...' : 'Continue' }}
            </Button>
            <p class="text-muted-foreground text-center text-xs text-pretty">
              By creating an account, you agree to the
              <RouterLink to="/terms" class="text-primary underline-offset-4 hover:underline">
                Terms of Service
              </RouterLink>
              and
              <RouterLink to="/privacy" class="text-primary underline-offset-4 hover:underline">
                Privacy Policy</RouterLink
              >.
            </p>
          </form>
        </CardContent>
        <CardFooter class="justify-center">
          <p class="text-muted-foreground text-sm">
            Already have an account?
            <RouterLink :to="loginTo" class="text-primary font-medium hover:underline">
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
