<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Loader2, MailCheck, MailX } from '@lucide/vue'
import { RouterLink, useRoute } from 'vue-router'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { useTurnstile } from '@/composables/useTurnstile'
import { ApiError, verifyEmail } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'

usePageMeta({ title: 'Verify your email', canonicalPath: '/verify-email', noindex: true })

const route = useRoute()
const turnstileEl = ref<HTMLElement>()
const { execute } = useTurnstile(turnstileEl)

// The emailed link lands here with ?token=…; consume it immediately on mount
// (one click = one consumption of the single-use token).
const state = ref<'verifying' | 'verified' | 'failed'>('verifying')
const error = ref<string | null>(null)

onMounted(async () => {
  const token = route.query.token
  if (typeof token !== 'string' || !token) {
    state.value = 'failed'
    error.value = 'This verification link is incomplete.'
    return
  }
  try {
    const captcha_token = (await execute()) ?? undefined
    await verifyEmail({ token, captcha_token })
    state.value = 'verified'
  } catch (err) {
    state.value = 'failed'
    error.value = err instanceof ApiError ? err.message : 'Something went wrong. Please try again.'
  }
})
</script>

<template>
  <div class="flex min-h-[calc(100vh-3.5rem)] items-center justify-center px-4 py-12">
    <div class="w-full max-w-sm">
      <Card class="w-full">
        <template v-if="state === 'verifying'">
          <CardHeader>
            <Loader2 class="text-muted-foreground size-8 animate-spin" />
            <CardTitle class="text-2xl">Verifying your email...</CardTitle>
            <CardDescription>This should only take a moment.</CardDescription>
          </CardHeader>
        </template>
        <template v-else-if="state === 'verified'">
          <CardHeader>
            <MailCheck class="text-primary size-8" />
            <CardTitle class="text-2xl">Email verified</CardTitle>
            <CardDescription>
              Your account is active — sign in to start tracking your collection.
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
            <MailX class="text-destructive size-8" />
            <CardTitle class="text-2xl">Verification failed</CardTitle>
            <CardDescription>
              <span role="alert">{{ error }}</span>
              Links expire after 24 hours and work once — try signing in, and we'll offer to send
              you a fresh one if your email still needs verifying.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Button as-child class="w-full">
              <RouterLink to="/login">Go to sign in</RouterLink>
            </Button>
          </CardContent>
        </template>
      </Card>
      <!-- CAPTCHA mount for the on-mount verification; invisible unless challenged. -->
      <div ref="turnstileEl" class="mt-4 flex justify-center empty:hidden"></div>
    </div>
  </div>
</template>
