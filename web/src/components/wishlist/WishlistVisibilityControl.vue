<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { Check, Copy, Globe, Lock } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import SetUsernameDialog from '@/components/collection/SetUsernameDialog.vue'
import {
  useSetWishlistVisibilityMutation,
  useWishlistVisibilityQuery,
} from '@/composables/useWishlistVisibility'
import { useAuthStore } from '@/stores/auth'
import { ApiError } from '@/lib/api'

// The per-game "make this wish list public" control (issue #493) — the wish-list twin of
// CollectionVisibilityControl, over the independent `wishlist_is_public` flag. A toggle plus,
// once public, the shareable `/u/{handle}/wishlist/{game}` URL. Making a wish list public
// requires a username first: if the user has none, the toggle opens the "choose a username"
// dialog and only enables sharing once it's saved. Per-game and independent of the collection's
// sharing — a public wish list never exposes the collection and vice versa.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')

const auth = useAuthStore()
const visibilityQuery = useWishlistVisibilityQuery(game)
const setVisibility = useSetWishlistVisibilityMutation()

const isPublic = computed(() => visibilityQuery.data.value?.public ?? false)
// Prefer the fresh handle from the auth store (updated the moment a username is set); fall
// back to the visibility response's handle.
const handle = computed(() => auth.user?.handle ?? visibilityQuery.data.value?.handle ?? null)
const shareUrl = computed(() =>
  handle.value ? `${window.location.origin}/u/${handle.value}/wishlist/${game.value}` : '',
)

const usernameDialogOpen = ref(false)
const error = ref<string | null>(null)
const copied = ref(false)
const busy = computed(() => setVisibility.isPending.value || visibilityQuery.isFetching.value)

async function setPublic(next: boolean) {
  error.value = null
  try {
    await setVisibility.mutateAsync({ game: game.value, patch: { public: next } })
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
  <div class="space-y-3">
    <div class="flex items-start justify-between gap-3">
      <div class="space-y-0.5">
        <p class="flex items-center gap-1.5 text-sm font-medium">
          <component :is="isPublic ? Globe : Lock" class="size-4" />
          {{ isPublic ? 'Public' : 'Private' }}
        </p>
        <p class="text-muted-foreground text-xs">
          <template v-if="isPublic">Anyone with the link can view this wish list.</template>
          <template v-else>Only you can see this wish list.</template>
        </p>
      </div>
      <!-- Controlled: the switch always reflects the server's `isPublic`; a click runs
           toggle(), which may open the username dialog instead of flipping. -->
      <Switch
        :checked="isPublic"
        :disabled="busy"
        aria-label="Wish list visibility"
        @update:checked="toggle"
      />
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
        :to="`/u/${handle}/wishlist/${game}`"
        class="text-primary inline-block text-xs underline underline-offset-2"
      >
        View public page →
      </RouterLink>
    </template>

    <p v-if="error" class="text-destructive text-xs">{{ error }}</p>
  </div>

  <SetUsernameDialog v-model:open="usernameDialogOpen" @saved="onUsernameSaved" />
</template>
