<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Check, LoaderCircle, TriangleAlert } from '@lucide/vue'
import { RouterLink, useRoute } from 'vue-router'
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useCreateAlertMutation } from '@/composables/useAlerts'
import { useAuthStore } from '@/stores/auth'
import { ApiError, type AlertDirection, type AlertFinish } from '@/lib/api'

// "Set price alert" dialog, opened from a card or sealed-product detail page (via
// SetPriceAlertButton in the shared detail body, so it reaches both the full page and the
// browse-grid modal). Controlled via `v-model:open`. Creating an alert is session-only, but
// the trigger is shown to everyone: a signed-out visitor opens this dialog too and gets an
// account-creation nudge instead of the form — the feature advertises itself. When signed in
// it creates one alert on the given target (offering only the finishes it's priced in — see
// `finishes`) and, on success, shows a confirmation with a link to manage alerts.
const props = defineProps<{
  game: string
  targetKind: 'card' | 'product'
  externalId: string
  name: string
  // The finishes this target is actually priced in (the caller derives them from the live
  // price columns). Only these are offered, and with a single entry the picker is hidden and
  // that finish is used implicitly — a regular-only card, or a sealed product (finish-less).
  finishes: AlertFinish[]
}>()

const auth = useAuthStore()
const route = useRoute()

// Where the nudge's two links send a signed-out visitor. Both carry `redirect: route.fullPath`
// — including a `?card=`/`?product=` modal param — so the detail reopens once they're back and
// they can set the alert without hunting for it again (RegisterView threads the redirect on
// through the completion flow, matching CollectionControls' sign-in prompt).
const loginTo = computed(() => ({ path: '/login', query: { redirect: route.fullPath } }))
const registerTo = computed(() => ({ path: '/register', query: { redirect: route.fullPath } }))

const open = defineModel<boolean>('open', { default: false })

const finish = ref<AlertFinish>('nonfoil')
const direction = ref<AlertDirection>('below')
const threshold = ref('')
const createError = ref<string | null>(null)
const created = ref(false)

const create = useCreateAlertMutation()

const FINISH_LABELS: Record<AlertFinish, string> = {
  nonfoil: 'Regular',
  foil: 'Foil',
  etched: 'Etched',
}

// Offer only the finishes the target is priced in (from `finishes`). Empty is defended against
// with a regular fallback so the form is always usable.
const finishOptions = computed<{ value: AlertFinish; label: string }[]>(() => {
  const list = props.finishes.length ? props.finishes : (['nonfoil'] as AlertFinish[])
  return list.map((value) => ({ value, label: FINISH_LABELS[value] }))
})

// With a single available finish there's nothing to choose: hide the picker and use it
// implicitly (sealed products are finish-less; a card may be priced in only one finish).
const showFinishSelect = computed(() => finishOptions.value.length > 1)

const canSubmit = computed(() => threshold.value.trim().length > 0 && !create.isPending.value)

// Reset the form each time the dialog opens so a prior attempt doesn't linger.
watch(open, (isOpen) => {
  if (isOpen) {
    finish.value = finishOptions.value[0]?.value ?? 'nonfoil'
    direction.value = 'below'
    threshold.value = ''
    createError.value = null
    created.value = false
  }
})

async function submit() {
  if (!canSubmit.value) return
  createError.value = null
  try {
    await create.mutateAsync({
      game: props.game,
      target_kind: props.targetKind,
      external_id: props.externalId,
      finish: finish.value,
      direction: direction.value,
      threshold: threshold.value.trim(),
    })
    created.value = true
  } catch (err) {
    createError.value =
      err instanceof ApiError ? err.message : 'Could not create the alert. Please try again.'
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="bg-background w-[min(92vw,28rem)] rounded-xl border p-6 shadow-xl">
      <DialogTitle class="text-lg font-semibold">Set a price alert</DialogTitle>
      <DialogDescription class="text-muted-foreground mt-1 text-sm">
        Get notified when <span class="font-medium">{{ name }}</span> crosses your price.
      </DialogDescription>

      <!-- Session still resolving: don't flash the sign-up nudge at an about-to-resolve user
        (the same guard CollectionControls uses before deciding nudge vs. controls). -->
      <div v-if="!auth.sessionResolved" class="mt-8 mb-4 flex justify-center">
        <LoaderCircle class="text-muted-foreground size-5 animate-spin" />
      </div>

      <!-- Signed out (resolved): advertise the feature and nudge account creation. Anyone can
        reach this dialog — the card/product pages are public — so a signed-out visitor lands
        here and is invited to sign up rather than shown a form they can't submit. -->
      <div v-else-if="!auth.isAuthenticated" class="mt-4 space-y-4">
        <p class="text-muted-foreground text-sm">
          Price alerts are free with a TCGLense account. Get a heads-up over Discord, Telegram, or
          email the moment <span class="text-foreground font-medium">{{ name }}</span> hits your
          target price.
        </p>
        <div class="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
          <RouterLink :to="loginTo" :class="buttonVariants({ variant: 'outline' })"
            >Sign in</RouterLink
          >
          <RouterLink :to="registerTo" :class="buttonVariants()">Create free account</RouterLink>
        </div>
      </div>

      <!-- Success state -->
      <div v-else-if="created" class="mt-4 space-y-4">
        <p class="flex items-center gap-2 text-sm text-emerald-600 dark:text-emerald-400">
          <Check class="size-4" /> Alert created.
        </p>
        <p class="text-muted-foreground text-sm">
          You'll be notified over your configured channels when it triggers.
        </p>
        <div class="flex justify-end gap-2">
          <DialogClose :class="buttonVariants({ variant: 'outline' })" type="button"
            >Done</DialogClose
          >
          <RouterLink to="/alerts" :class="buttonVariants()">Manage alerts</RouterLink>
        </div>
      </div>

      <!-- Form -->
      <form v-else class="mt-4 space-y-4" @submit.prevent="submit">
        <!-- Finish sits beside the direction only when there's a choice to make; otherwise the
          direction takes the full row and the single available finish is used implicitly. -->
        <div class="grid gap-3" :class="showFinishSelect ? 'grid-cols-2' : 'grid-cols-1'">
          <div v-if="showFinishSelect" class="space-y-1.5">
            <Label>Finish</Label>
            <Select v-model="finish">
              <SelectTrigger aria-label="Finish"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem v-for="opt in finishOptions" :key="opt.value" :value="opt.value">
                  {{ opt.label }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div class="space-y-1.5">
            <Label>When price is</Label>
            <Select v-model="direction">
              <SelectTrigger aria-label="Direction"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="below">At or below</SelectItem>
                <SelectItem value="above">At or above</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>

        <div class="space-y-1.5">
          <Label for="alert-threshold">Threshold (USD)</Label>
          <div class="flex items-center gap-2">
            <span class="text-muted-foreground">$</span>
            <Input
              id="alert-threshold"
              v-model="threshold"
              inputmode="decimal"
              placeholder="e.g. 5.00"
              autocomplete="off"
            />
          </div>
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
          <DialogClose :class="buttonVariants({ variant: 'outline' })" type="button"
            >Cancel</DialogClose
          >
          <Button type="submit" :disabled="!canSubmit">
            <LoaderCircle v-if="create.isPending.value" class="animate-spin" />
            Create alert
          </Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>
