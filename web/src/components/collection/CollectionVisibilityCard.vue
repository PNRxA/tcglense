<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { Check, Copy, Globe, Lock } from '@lucide/vue'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import SetUsernameDialog from '@/components/collection/SetUsernameDialog.vue'
import {
  useCollectionVisibilityQuery,
  useSetCollectionVisibilityMutation,
} from '@/composables/useCollectionVisibility'
import { useAuthStore } from '@/stores/auth'
import { ApiError } from '@/lib/api'

// The per-game "make this collection public" control (issues #361/#362), mounted on the
// owner's collection landing. A toggle plus, once public, the shareable `/u/{handle}/{game}`
// URL. Making a collection public requires a username first: if the user has none, the
// toggle opens the "choose a username" dialog and only enables sharing once it's saved.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')

const auth = useAuthStore()
const visibilityQuery = useCollectionVisibilityQuery(game)
const setVisibility = useSetCollectionVisibilityMutation()

const isPublic = computed(() => visibilityQuery.data.value?.public ?? false)
// Prefer the fresh handle from the auth store (updated the moment a username is set); fall
// back to the visibility response's handle.
const handle = computed(() => auth.user?.handle ?? visibilityQuery.data.value?.handle ?? null)
const shareUrl = computed(() =>
  handle.value ? `${window.location.origin}/u/${handle.value}/${game.value}` : '',
)

const usernameDialogOpen = ref(false)
const error = ref<string | null>(null)
const copied = ref(false)
const busy = computed(() => setVisibility.isPending.value || visibilityQuery.isFetching.value)

async function setPublic(next: boolean) {
  error.value = null
  try {
    await setVisibility.mutateAsync({ game: game.value, public: next })
  } catch (err) {
    error.value =
      err instanceof ApiError ? err.message : 'Could not update sharing. Please try again.'
  }
}

async function toggle() {
  if (busy.value) return
  if (isPublic.value) {
    await setPublic(false)
    return
  }
  // Making public needs a username first (the server 409s otherwise): prompt for one
  // rather than round-tripping a guaranteed conflict.
  if (!auth.user?.username) {
    error.value = null
    usernameDialogOpen.value = true
    return
  }
  await setPublic(true)
}

// The username dialog saved — finish the "make public" the toggle started.
function onUsernameSaved() {
  void setPublic(true)
}

async function copyShareUrl() {
  if (!shareUrl.value) return
  try {
    await navigator.clipboard.writeText(shareUrl.value)
    copied.value = true
    setTimeout(() => {
      copied.value = false
    }, 2000)
  } catch {
    // Clipboard access can be denied (insecure context / permissions); the URL stays
    // visible for manual selection, so there's nothing to surface here.
  }
}
</script>

<template>
  <Card class="mt-5 max-w-md">
    <CardHeader class="pb-3">
      <CardTitle class="flex items-center gap-2 text-base">
        <component :is="isPublic ? Globe : Lock" class="size-4" />
        Public collection
      </CardTitle>
      <CardDescription>
        <template v-if="isPublic">Anyone with the link can view this collection.</template>
        <template v-else>Only you can see this collection.</template>
      </CardDescription>
    </CardHeader>
    <CardContent class="space-y-3">
      <div class="flex items-center justify-between gap-3">
        <span class="text-sm font-medium">{{ isPublic ? 'Public' : 'Private' }}</span>
        <button
          type="button"
          role="switch"
          :aria-checked="isPublic"
          aria-label="Make this collection public"
          :disabled="busy"
          class="focus-visible:ring-ring/50 inline-flex h-6 w-11 shrink-0 items-center rounded-full border transition-colors outline-none focus-visible:ring-2 disabled:cursor-not-allowed disabled:opacity-50"
          :class="isPublic ? 'bg-primary border-primary' : 'bg-input border-input'"
          @click="toggle"
        >
          <span
            class="bg-background size-5 rounded-full shadow-sm transition-transform"
            :class="isPublic ? 'translate-x-5' : 'translate-x-0.5'"
          />
        </button>
      </div>

      <!-- Share row, shown once public: the link + a copy button + a link to the live page. -->
      <template v-if="isPublic && handle">
        <div class="flex items-center gap-2">
          <code
            class="bg-background min-w-0 flex-1 overflow-x-auto rounded-md border px-2 py-1.5 font-mono text-xs whitespace-nowrap"
            >{{ shareUrl }}</code
          >
          <Button variant="outline" size="sm" type="button" @click="copyShareUrl">
            <component :is="copied ? Check : Copy" class="size-4" />
            {{ copied ? 'Copied' : 'Copy' }}
          </Button>
        </div>
        <RouterLink
          :to="`/u/${handle}/${game}`"
          class="text-primary inline-block text-xs underline underline-offset-2"
        >
          View public page →
        </RouterLink>
      </template>

      <p v-if="error" class="text-destructive text-xs">{{ error }}</p>
    </CardContent>
  </Card>

  <SetUsernameDialog v-model:open="usernameDialogOpen" @saved="onUsernameSaved" />
</template>
