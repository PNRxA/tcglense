<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Check, LoaderCircle, TriangleAlert } from '@lucide/vue'
import { RouterLink } from 'vue-router'
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
import { ApiError, type AlertDirection, type AlertFinish } from '@/lib/api'

// "Set price alert" dialog, opened from a card or sealed-product detail page. Controlled via
// `v-model:open`. It creates one alert on the given target (foil/etched offered per kind) and,
// on success, shows a confirmation with a link to manage alerts.
const props = defineProps<{
  game: string
  targetKind: 'card' | 'product'
  externalId: string
  name: string
}>()

const open = defineModel<boolean>('open', { default: false })

const finish = ref<AlertFinish>('nonfoil')
const direction = ref<AlertDirection>('below')
const threshold = ref('')
const createError = ref<string | null>(null)
const created = ref(false)

const create = useCreateAlertMutation()

// Sealed products have no etched finish; cards do.
const finishOptions = computed<{ value: AlertFinish; label: string }[]>(() => {
  const base: { value: AlertFinish; label: string }[] = [
    { value: 'nonfoil', label: 'Regular' },
    { value: 'foil', label: 'Foil' },
  ]
  if (props.targetKind === 'card') base.push({ value: 'etched', label: 'Etched' })
  return base
})

const canSubmit = computed(() => threshold.value.trim().length > 0 && !create.isPending.value)

// Reset the form each time the dialog opens so a prior attempt doesn't linger.
watch(open, (isOpen) => {
  if (isOpen) {
    finish.value = 'nonfoil'
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

      <!-- Success state -->
      <div v-if="created" class="mt-4 space-y-4">
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
        <div class="grid grid-cols-2 gap-3">
          <div class="space-y-1.5">
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
