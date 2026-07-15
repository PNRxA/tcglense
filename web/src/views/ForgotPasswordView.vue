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
import { useTurnstile } from '@/composables/useTurnstile'
import { ApiError, forgotPassword } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'

usePageMeta({ title: 'Forgot password', canonicalPath: '/forgot-password', noindex: true })

const email = ref('')
const error = ref<string | null>(null)
const loading = ref(false)
// The server answers generically whether or not the address has an account, so
// the success state is the whole outcome — there is nothing else to reveal.
const submitted = ref(false)
const turnstileEl = ref<HTMLElement>()
const { execute } = useTurnstile(turnstileEl)

async function onSubmit() {
  error.value = null
  loading.value = true
  try {
    const captcha_token = (await execute()) ?? undefined
    await forgotPassword({ email: email.value, captcha_token })
    submitted.value = true
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
        <CardTitle class="text-2xl">Reset your password</CardTitle>
        <CardDescription>
          {{
            submitted
              ? 'If an account exists for that address, a reset link is on its way.'
              : "Enter your account's email address and we'll send you a reset link."
          }}
        </CardDescription>
      </CardHeader>
      <CardContent>
        <p v-if="submitted" class="text-muted-foreground text-sm" role="status">
          The link is valid for 1 hour. Check your spam folder if it doesn't show up in a minute or
          two.
        </p>
        <form
          v-else
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
              :aria-describedby="error ? 'forgot-error' : undefined"
            />
          </div>
          <p v-if="error" id="forgot-error" class="text-destructive text-sm" role="alert">
            {{ error }}
          </p>
          <div ref="turnstileEl" class="empty:hidden"></div>
          <Button type="submit" class="w-full" :disabled="loading">
            <Loader2 v-if="loading" class="animate-spin" />
            {{ loading ? 'Sending link...' : 'Send reset link' }}
          </Button>
        </form>
      </CardContent>
      <CardFooter class="justify-center">
        <p class="text-muted-foreground text-sm">
          Remembered it?
          <RouterLink to="/login" class="text-primary font-medium hover:underline">
            Sign in
          </RouterLink>
        </p>
      </CardFooter>
    </Card>
  </div>
</template>
