<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Check, LoaderCircle, TriangleAlert } from '@lucide/vue'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button, buttonVariants } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useSetUsernameMutation, useUsernameAvailabilityQuery } from '@/composables/useUsername'
import { ApiError } from '@/lib/api'

// "Choose a username" dialog (issue #362), opened when a user makes a collection public
// without one. Controlled open via `v-model:open`; emits `saved` on success so the parent
// can chain the "make public" it started. Live feedback: a cheap client-side pre-check
// gates a debounced server availability/profanity check.
const open = defineModel<boolean>('open', { default: false })
const emit = defineEmits<{ saved: [] }>()

const username = ref('')
const debounced = ref('')
const saveError = ref<string | null>(null)

const setUsername = useSetUsernameMutation()

// Mirror the server's charset/length rules cheaply so an obviously-invalid name never fires
// the availability request (and gets instant guidance).
const USERNAME_RE = /^[A-Za-z0-9_]+$/
const trimmed = computed(() => username.value.trim())
const localValid = computed(
  () => trimmed.value.length >= 3 && trimmed.value.length <= 20 && USERNAME_RE.test(trimmed.value),
)
const localError = computed(() => {
  if (!trimmed.value) return null
  if (trimmed.value.length < 3 || trimmed.value.length > 20) return 'Must be 3–20 characters.'
  if (!USERNAME_RE.test(trimmed.value)) return 'Letters, numbers, and underscores only.'
  return null
})

// Debounce the availability lookup ~300ms (no @vueuse — a hand-rolled timer).
let timer: ReturnType<typeof setTimeout> | undefined
watch(username, (value) => {
  if (timer) clearTimeout(timer)
  timer = setTimeout(() => {
    debounced.value = value.trim()
  }, 300)
})

const availabilityEnabled = computed(() => localValid.value && debounced.value.length >= 3)
const availability = useUsernameAvailabilityQuery(debounced, availabilityEnabled)

const checking = computed(() => availabilityEnabled.value && availability.isFetching.value)
const available = computed(
  () => availabilityEnabled.value && availability.data.value?.valid === true,
)
const serverReason = computed(() =>
  availabilityEnabled.value && availability.data.value && !availability.data.value.valid
    ? availability.data.value.reason
    : null,
)

const canSave = computed(() => available.value && !setUsername.isPending.value)

// Clear the form each time the dialog opens so a prior attempt doesn't linger.
watch(open, (isOpen) => {
  if (isOpen) {
    username.value = ''
    debounced.value = ''
    saveError.value = null
  }
})

async function submit() {
  if (!canSave.value) return
  saveError.value = null
  try {
    await setUsername.mutateAsync({ username: trimmed.value })
    open.value = false
    emit('saved')
  } catch (err) {
    saveError.value =
      err instanceof ApiError ? err.message : 'Could not set your username. Please try again.'
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="bg-background w-[min(92vw,28rem)] rounded-xl border p-6 shadow-xl">
      <DialogTitle class="text-lg font-semibold">Choose a username</DialogTitle>
      <DialogDescription class="text-muted-foreground mt-1 text-sm">
        Your public collections are shared at
        <code class="font-mono text-xs">/u/your-name</code>. A #0000 tag is added
        automatically, so a common name is fine.
      </DialogDescription>

      <form class="mt-4 space-y-4" @submit.prevent="submit">
        <div class="space-y-1.5">
          <Label for="username-input">Username</Label>
          <Input
            id="username-input"
            v-model="username"
            placeholder="e.g. ada_lovelace"
            maxlength="20"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
          />
          <!-- Live feedback: local pre-check, then the server's validity/profanity check. -->
          <p v-if="localError" class="text-destructive text-xs">{{ localError }}</p>
          <p v-else-if="checking" class="text-muted-foreground text-xs">Checking…</p>
          <p v-else-if="serverReason" class="text-destructive text-xs">{{ serverReason }}</p>
          <p
            v-else-if="available"
            class="flex items-center gap-1 text-xs text-emerald-600 dark:text-emerald-400"
          >
            <Check class="size-3.5" /> Looks good
          </p>
          <p v-else class="text-muted-foreground text-xs">
            3–20 characters — letters, numbers, and underscores.
          </p>
        </div>

        <p
          v-if="saveError"
          class="text-destructive flex items-start gap-1.5 text-sm"
          role="alert"
        >
          <TriangleAlert class="mt-0.5 size-4 shrink-0" />
          <span>{{ saveError }}</span>
        </p>

        <div class="flex justify-end gap-2 pt-1">
          <DialogClose :class="buttonVariants({ variant: 'outline' })" type="button">
            Cancel
          </DialogClose>
          <Button type="submit" :disabled="!canSave">
            <LoaderCircle v-if="setUsername.isPending.value" class="animate-spin" />
            Save username
          </Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>
