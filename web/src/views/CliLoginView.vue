<script setup lang="ts">
import { computed, ref } from 'vue'
import { Loader2 } from '@lucide/vue'
import { useRoute, useRouter } from 'vue-router'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { ApiError } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'
import { useCliAuthorizeMutation } from '@/composables/useCliAuth'

usePageMeta({ title: 'Authorize the CLI', canonicalPath: '/cli-login', noindex: true })

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const authorize = useCliAuthorizeMutation()

const error = ref<string | null>(null)
const redirecting = ref(false)

function queryString(key: string): string {
  const v = route.query[key]
  return typeof v === 'string' ? v : ''
}

// The CLI's loopback redirect target. Only ever an http loopback address — never
// route this through the app router or the `redirect` guard param (those reject
// off-origin URLs); we hand it straight to `window.location` after validating it.
const loopbackRedirect = computed<string | null>(() => {
  const raw = queryString('redirect_uri')
  if (!raw) return null
  try {
    const url = new URL(raw)
    const host = url.hostname
    const isLoopback =
      host === '127.0.0.1' || host === 'localhost' || host === '::1' || host === '[::1]'
    if (url.protocol === 'http:' && isLoopback) return raw
  } catch {
    // Not a URL — treated as missing below.
  }
  return null
})

const codeChallenge = computed(() => queryString('code_challenge'))
const state = computed(() => queryString('state'))
const deviceName = computed(() => {
  const name = queryString('name').trim()
  return name.length > 0 ? name.slice(0, 100) : 'a command-line device'
})

const paramError = computed<string | null>(() => {
  if (!loopbackRedirect.value) return 'This sign-in link is missing a valid local redirect address.'
  if (!/^[0-9a-fA-F]{64}$/.test(codeChallenge.value)) {
    return 'This sign-in link is missing its security challenge.'
  }
  if (!state.value) return 'This sign-in link is incomplete.'
  return null
})

function returnUrl(params: Record<string, string>): string {
  // loopbackRedirect is validated non-null before this is called.
  const url = new URL(loopbackRedirect.value as string)
  for (const [key, value] of Object.entries(params)) url.searchParams.set(key, value)
  return url.toString()
}

async function onAuthorize() {
  if (paramError.value) return
  error.value = null
  try {
    const res = await authorize.mutateAsync({
      code_challenge: codeChallenge.value,
      client_name: deviceName.value,
    })
    redirecting.value = true
    window.location.href = returnUrl({ code: res.code, state: state.value })
  } catch (err) {
    error.value =
      err instanceof ApiError ? err.message : 'Could not authorize the device. Please try again.'
  }
}

function onCancel() {
  if (!loopbackRedirect.value) {
    void router.push('/')
    return
  }
  redirecting.value = true
  // Tell the waiting CLI the request was declined, so it exits cleanly.
  window.location.href = returnUrl({ error: 'access_denied', state: state.value })
}
</script>

<template>
  <div class="flex min-h-[calc(100vh-3.5rem)] items-center justify-center px-4 py-12">
    <Card class="w-full max-w-sm">
      <template v-if="redirecting">
        <CardHeader>
          <CardTitle class="text-2xl">Returning to the CLI…</CardTitle>
          <CardDescription> You can close this tab and return to your terminal. </CardDescription>
        </CardHeader>
        <CardContent class="flex justify-center py-4">
          <Loader2 class="text-muted-foreground size-6 animate-spin" />
        </CardContent>
      </template>

      <template v-else-if="paramError">
        <CardHeader>
          <CardTitle class="text-2xl">Invalid sign-in link</CardTitle>
          <CardDescription>{{ paramError }}</CardDescription>
        </CardHeader>
        <CardContent>
          <p class="text-muted-foreground text-sm">
            Start again from your terminal with
            <code class="bg-muted rounded px-1 py-0.5">tcglense login</code>.
          </p>
        </CardContent>
      </template>

      <template v-else>
        <CardHeader>
          <CardTitle class="text-2xl">Authorize the CLI</CardTitle>
          <CardDescription>
            <strong class="text-foreground">{{ deviceName }}</strong> wants to sign in to your
            TCGLense account.
          </CardDescription>
        </CardHeader>
        <CardContent class="flex flex-col gap-3">
          <p v-if="auth.user" class="text-muted-foreground text-sm">
            Signed in as <span class="text-foreground font-medium">{{ auth.user.email }}</span
            >.
          </p>
          <p class="text-muted-foreground text-sm">
            Approving grants this device full access to your account — the same as signing in,
            including managing API keys. Only continue if you just started
            <code class="bg-muted rounded px-1 py-0.5">tcglense login</code> yourself.
          </p>
          <p v-if="error" class="text-destructive text-sm" role="alert">{{ error }}</p>
        </CardContent>
        <CardFooter class="flex gap-2">
          <Button
            variant="outline"
            class="flex-1"
            :disabled="authorize.isPending.value"
            @click="onCancel"
          >
            Cancel
          </Button>
          <Button class="flex-1" :disabled="authorize.isPending.value" @click="onAuthorize">
            <Loader2 v-if="authorize.isPending.value" class="animate-spin" />
            {{ authorize.isPending.value ? 'Authorizing…' : 'Authorize' }}
          </Button>
        </CardFooter>
      </template>
    </Card>
  </div>
</template>
