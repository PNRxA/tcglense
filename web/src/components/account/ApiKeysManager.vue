<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Check, Copy, KeyRound, LoaderCircle, Plus, Trash2, TriangleAlert } from '@lucide/vue'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Button, buttonVariants } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  useApiKeysQuery,
  useCreateApiKeyMutation,
  useRevokeApiKeyMutation,
} from '@/composables/useApiKeys'
import { ApiError, type ApiKeyScope } from '@/lib/api'
import type { ApiKeyInfo, CreatedApiKey } from '@/lib/api/generated'

// The signed-in user's API-key management panel (issue #284): create a key (its plaintext
// shown exactly once), see active keys with their scope/usage, and revoke. Mounted on the
// profile page for a logged-in user, so the composables always have a session.

const keysQuery = useApiKeysQuery()
const createMutation = useCreateApiKeyMutation()
const revokeMutation = useRevokeApiKeyMutation()

const keys = computed<ApiKeyInfo[]>(() => keysQuery.data.value?.data ?? [])

// ---- Create flow ----
const createOpen = ref(false)
const name = ref('')
const scope = ref<ApiKeyScope>('read_write')
const expiry = ref<'never' | '30' | '90' | '365'>('never')
const createError = ref<string | null>(null)

// The one-time plaintext of the just-created key. Non-null shows the copy-me-now banner;
// it lives in the component (never re-fetchable) and is cleared when dismissed.
const createdKey = ref<CreatedApiKey | null>(null)
const copied = ref(false)

const canCreate = computed(() => name.value.trim().length > 0 && !createMutation.isPending.value)

function resetCreateForm() {
  name.value = ''
  scope.value = 'read_write'
  expiry.value = 'never'
  createError.value = null
}

// Reset the form every time the dialog opens, so a prior failed attempt's error and
// typed values don't linger across opens (matches ImportCollectionDialog).
watch(createOpen, (isOpen) => {
  if (isOpen) resetCreateForm()
})

function expiresInDays(): number | null {
  return expiry.value === 'never' ? null : Number(expiry.value)
}

async function submitCreate() {
  if (!canCreate.value) return
  createError.value = null
  try {
    const created = await createMutation.mutateAsync({
      name: name.value.trim(),
      scope: scope.value,
      expiresInDays: expiresInDays(),
    })
    createdKey.value = created
    copied.value = false
    createOpen.value = false
    resetCreateForm()
  } catch (err) {
    createError.value =
      err instanceof ApiError ? err.message : 'Could not create the key. Please try again.'
  }
}

async function copyKey() {
  if (!createdKey.value) return
  try {
    await navigator.clipboard.writeText(createdKey.value.key)
    copied.value = true
    setTimeout(() => {
      copied.value = false
    }, 2000)
  } catch {
    // Clipboard access can be denied (insecure context / permissions); the key stays
    // visible for manual selection, so there's nothing to surface here.
  }
}

function dismissCreatedKey() {
  createdKey.value = null
  copied.value = false
  // Also drop the plaintext from the vue-query mutation cache so the one-time secret
  // doesn't linger in memory after it's dismissed.
  createMutation.reset()
}

// ---- Revoke flow ----
const revokeTarget = ref<ApiKeyInfo | null>(null)
// The target's name held separately so the confirmation text stays stable while the
// dialog animates out (revokeTarget is cleared synchronously on close).
const revokeName = ref('')
const revokeError = ref<string | null>(null)
const revokeOpen = computed({
  get: () => revokeTarget.value !== null,
  set: (open: boolean) => {
    if (!open) {
      revokeTarget.value = null
      revokeError.value = null
    }
  },
})

function openRevoke(key: ApiKeyInfo) {
  revokeTarget.value = key
  revokeName.value = key.name
  revokeError.value = null
}

async function confirmRevoke() {
  const target = revokeTarget.value
  if (!target) return
  revokeError.value = null
  try {
    await revokeMutation.mutateAsync(target.id)
    // If the just-revoked key is the one still shown in the copy banner, clear it too.
    if (createdKey.value?.id === target.id) dismissCreatedKey()
    revokeTarget.value = null
  } catch (err) {
    revokeError.value =
      err instanceof ApiError ? err.message : 'Could not revoke the key. Please try again.'
  }
}

// ---- Formatting ----
function formatDate(ts: string | null): string {
  if (!ts) return '—'
  const date = new Date(ts)
  if (Number.isNaN(date.getTime())) return '—'
  return date.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })
}

function scopeLabel(value: string): string {
  return value === 'read_write' ? 'Read & write' : 'Read-only'
}

// The native <select> styling, matching the import dialog's inputs.
const selectClass =
  'border-input dark:bg-input/30 flex h-9 w-full rounded-md border bg-transparent px-3 text-sm ' +
  'shadow-xs outline-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px]'
</script>

<template>
  <Card>
    <CardHeader>
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div class="min-w-0">
          <CardTitle class="flex items-center gap-2 text-xl">
            <KeyRound class="size-5" />
            API keys
          </CardTitle>
          <CardDescription class="mt-1">
            Access the public API from your own scripts. See the
            <RouterLink to="/docs" class="text-primary underline underline-offset-2">
              API reference
            </RouterLink>
            for endpoints and examples.
          </CardDescription>
        </div>

        <Dialog v-model:open="createOpen">
          <DialogTrigger :class="buttonVariants({ variant: 'outline', size: 'sm' })">
            <Plus />
            New key
          </DialogTrigger>
          <DialogContent class="bg-background w-[min(92vw,28rem)] rounded-xl border p-6 shadow-xl">
            <DialogTitle class="text-lg font-semibold">Create an API key</DialogTitle>
            <DialogDescription class="text-muted-foreground mt-1 text-sm">
              The full key is shown only once, right after you create it. Store it somewhere safe —
              you can't see it again.
            </DialogDescription>

            <form class="mt-4 space-y-4" @submit.prevent="submitCreate">
              <div class="space-y-1.5">
                <Label for="api-key-name">Name</Label>
                <Input
                  id="api-key-name"
                  v-model="name"
                  placeholder="e.g. price-tracker"
                  maxlength="100"
                  autocomplete="off"
                />
                <p class="text-muted-foreground text-xs">
                  A label so you can tell your keys apart.
                </p>
              </div>

              <div class="space-y-1.5">
                <Label for="api-key-scope">Access</Label>
                <select id="api-key-scope" v-model="scope" :class="selectClass">
                  <option value="read_write">Read &amp; write — read and modify your data</option>
                  <option value="read">Read-only — read your data, no changes</option>
                </select>
              </div>

              <div class="space-y-1.5">
                <Label for="api-key-expiry">Expiry</Label>
                <select id="api-key-expiry" v-model="expiry" :class="selectClass">
                  <option value="never">Never expires</option>
                  <option value="30">30 days</option>
                  <option value="90">90 days</option>
                  <option value="365">1 year</option>
                </select>
              </div>

              <p
                v-if="createError"
                class="text-destructive flex items-start gap-1.5 text-sm"
                role="alert"
              >
                <TriangleAlert class="mt-0.5 size-4 shrink-0" />
                <span>{{ createError }}</span>
              </p>

              <div class="flex justify-end gap-2 pt-1">
                <DialogClose :class="buttonVariants({ variant: 'outline' })" type="button">
                  Cancel
                </DialogClose>
                <Button type="submit" :disabled="!canCreate">
                  <LoaderCircle v-if="createMutation.isPending.value" class="animate-spin" />
                  Create key
                </Button>
              </div>
            </form>
          </DialogContent>
        </Dialog>
      </div>
    </CardHeader>

    <CardContent class="space-y-4">
      <!-- One-time plaintext banner for a freshly created key. -->
      <div v-if="createdKey" class="border-primary/40 bg-primary/5 space-y-2 rounded-lg border p-4">
        <div class="flex items-center gap-2 text-sm font-medium">
          <Check class="text-primary size-4" />
          Key “{{ createdKey.name }}” created
        </div>
        <p class="text-muted-foreground text-xs">Copy it now — this is the only time it's shown.</p>
        <div class="flex items-center gap-2">
          <code
            class="bg-background min-w-0 flex-1 overflow-x-auto rounded-md border px-2 py-1.5 font-mono text-xs whitespace-nowrap"
            >{{ createdKey.key }}</code
          >
          <Button variant="outline" size="sm" type="button" @click="copyKey">
            <component :is="copied ? Check : Copy" class="size-4" />
            {{ copied ? 'Copied' : 'Copy' }}
          </Button>
        </div>
        <div class="flex justify-end">
          <Button variant="ghost" size="sm" type="button" @click="dismissCreatedKey">Done</Button>
        </div>
      </div>

      <!-- Loading -->
      <p v-if="keysQuery.isPending.value" class="text-muted-foreground text-sm">Loading keys…</p>

      <!-- Error -->
      <p
        v-else-if="keysQuery.isError.value"
        class="text-destructive flex items-center gap-1.5 text-sm"
        role="alert"
      >
        <TriangleAlert class="size-4" />
        Couldn't load your API keys.
      </p>

      <!-- Empty -->
      <p v-else-if="keys.length === 0" class="text-muted-foreground text-sm">
        You don't have any API keys yet. Create one to use the API programmatically.
      </p>

      <!-- List -->
      <ul v-else class="divide-border divide-y">
        <li
          v-for="key in keys"
          :key="key.id"
          class="flex flex-wrap items-center justify-between gap-3 py-3"
        >
          <div class="min-w-0">
            <div class="flex items-center gap-2">
              <span class="truncate text-sm font-medium">{{ key.name }}</span>
              <span
                class="bg-muted text-muted-foreground rounded px-1.5 py-0.5 text-xs whitespace-nowrap"
              >
                {{ scopeLabel(key.scope) }}
              </span>
            </div>
            <div class="text-muted-foreground mt-0.5 flex flex-wrap gap-x-3 gap-y-0.5 text-xs">
              <span class="font-mono">{{ key.key_prefix }}…</span>
              <span>Created {{ formatDate(key.created_at) }}</span>
              <span>
                {{ key.last_used_at ? `Last used ${formatDate(key.last_used_at)}` : 'Never used' }}
              </span>
              <span v-if="key.expires_at">Expires {{ formatDate(key.expires_at) }}</span>
            </div>
          </div>
          <Button
            variant="ghost"
            size="sm"
            type="button"
            class="text-muted-foreground hover:text-destructive"
            @click="openRevoke(key)"
          >
            <Trash2 class="size-4" />
            Revoke
          </Button>
        </li>
      </ul>
    </CardContent>

    <!-- Revoke confirmation -->
    <Dialog v-model:open="revokeOpen">
      <DialogContent class="bg-background w-[min(92vw,26rem)] rounded-xl border p-6 shadow-xl">
        <DialogTitle class="text-lg font-semibold">Revoke this key?</DialogTitle>
        <DialogDescription class="text-muted-foreground mt-1 text-sm">
          “{{ revokeName }}” will stop working immediately. Any script using it will need a new key.
          This can't be undone.
        </DialogDescription>
        <p
          v-if="revokeError"
          class="text-destructive mt-3 flex items-start gap-1.5 text-sm"
          role="alert"
        >
          <TriangleAlert class="mt-0.5 size-4 shrink-0" />
          <span>{{ revokeError }}</span>
        </p>
        <div class="mt-5 flex justify-end gap-2">
          <DialogClose :class="buttonVariants({ variant: 'outline' })" type="button">
            Cancel
          </DialogClose>
          <Button
            variant="destructive"
            type="button"
            :disabled="revokeMutation.isPending.value"
            @click="confirmRevoke"
          >
            <LoaderCircle v-if="revokeMutation.isPending.value" class="animate-spin" />
            Revoke key
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  </Card>
</template>
