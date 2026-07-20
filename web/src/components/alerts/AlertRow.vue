<script setup lang="ts">
import { computed, ref } from 'vue'
import { ArrowDown, ArrowUp, LoaderCircle, Pencil, Trash2, X } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useDeleteAlertMutation, useUpdateAlertMutation } from '@/composables/useAlerts'
import { ApiError, type PriceAlert } from '@/lib/api'

const props = defineProps<{ alert: PriceAlert }>()

const update = useUpdateAlertMutation()
const remove = useDeleteAlertMutation()

const editing = ref(false)
const editDirection = ref<'below' | 'above'>('below')
const editThreshold = ref('')
const rowError = ref<string | null>(null)

// The detail-page link for the target: cards live under /cards, sealed products under /sealed.
const targetLink = computed(() => {
  if (!props.alert.target.external_id) return null
  return props.alert.target.kind === 'card'
    ? `/cards/${props.alert.game}/cards/${props.alert.target.external_id}`
    : `/sealed/${props.alert.game}/${props.alert.target.external_id}`
})

const finishLabel = computed(() =>
  props.alert.finish === 'nonfoil' ? '' : ` ${props.alert.finish}`,
)

function startEdit() {
  editDirection.value = props.alert.direction === 'above' ? 'above' : 'below'
  editThreshold.value = props.alert.threshold
  rowError.value = null
  editing.value = true
}

async function saveEdit() {
  rowError.value = null
  try {
    await update.mutateAsync({
      id: props.alert.id,
      // ts-rs renders the optional fields as required-with-null; send null for the ones
      // this edit leaves unchanged (the backend treats null + absent alike).
      body: {
        finish: null,
        direction: editDirection.value,
        threshold: editThreshold.value.trim(),
        is_active: null,
      },
    })
    editing.value = false
  } catch (err) {
    rowError.value = err instanceof ApiError ? err.message : 'Could not save the alert.'
  }
}

async function toggleActive(next: boolean) {
  rowError.value = null
  try {
    await update.mutateAsync({
      id: props.alert.id,
      body: { finish: null, direction: null, threshold: null, is_active: next },
    })
  } catch (err) {
    rowError.value = err instanceof ApiError ? err.message : 'Could not update the alert.'
  }
}

async function onDelete() {
  rowError.value = null
  try {
    await remove.mutateAsync(props.alert.id)
  } catch (err) {
    rowError.value = err instanceof ApiError ? err.message : 'Could not delete the alert.'
  }
}
</script>

<template>
  <div
    class="flex flex-wrap items-center gap-3 rounded-lg border p-3"
    :class="{ 'opacity-60': !alert.is_active }"
  >
    <!-- Target image -->
    <component
      :is="targetLink ? RouterLink : 'div'"
      :to="targetLink ?? undefined"
      class="bg-muted h-14 w-10 shrink-0 overflow-hidden rounded"
    >
      <img
        v-if="alert.target.image_url"
        :src="alert.target.image_url"
        :alt="alert.target.name"
        class="h-full w-full object-cover"
        loading="lazy"
      />
    </component>

    <!-- Target + condition -->
    <div class="min-w-0 flex-1">
      <component
        :is="targetLink ? RouterLink : 'span'"
        :to="targetLink ?? undefined"
        class="block truncate font-medium hover:underline"
      >
        {{ alert.target.name
        }}<span class="text-muted-foreground capitalize">{{ finishLabel }}</span>
      </component>
      <p class="text-muted-foreground truncate text-xs uppercase">{{ alert.target.set_code }}</p>

      <div v-if="!editing" class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
        <span class="inline-flex items-center gap-1">
          <ArrowDown v-if="alert.direction === 'below'" class="size-3.5" />
          <ArrowUp v-else class="size-3.5" />
          {{ alert.direction === 'below' ? 'At or below' : 'At or above' }}
          <span class="font-medium">${{ alert.threshold }}</span>
        </span>
        <span class="text-muted-foreground text-xs">
          now
          <span class="text-foreground font-medium">
            {{ alert.target.current_price ? `$${alert.target.current_price}` : '—' }}
          </span>
        </span>
        <span
          v-if="alert.triggered"
          class="rounded-full bg-amber-100 px-2 py-0.5 text-xs font-medium text-amber-700 dark:bg-amber-500/15 dark:text-amber-400"
        >
          Triggered
        </span>
      </div>

      <!-- Inline edit -->
      <div v-else class="mt-2 flex flex-wrap items-center gap-2">
        <Select v-model="editDirection">
          <SelectTrigger class="h-8 w-36" aria-label="Direction">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="below">At or below</SelectItem>
            <SelectItem value="above">At or above</SelectItem>
          </SelectContent>
        </Select>
        <div class="flex items-center gap-1">
          <span class="text-muted-foreground text-sm">$</span>
          <Input
            v-model="editThreshold"
            inputmode="decimal"
            class="h-8 w-24"
            aria-label="Threshold in USD"
          />
        </div>
        <Button size="sm" :disabled="update.isPending.value" @click="saveEdit">
          <LoaderCircle v-if="update.isPending.value" class="animate-spin" />
          Save
        </Button>
        <Button size="sm" variant="ghost" @click="editing = false"><X class="size-4" /></Button>
      </div>

      <p v-if="rowError" class="text-destructive mt-1 text-xs" role="alert">{{ rowError }}</p>
    </div>

    <!-- Controls -->
    <div v-if="!editing" class="flex items-center gap-1">
      <Switch
        :checked="alert.is_active"
        :disabled="update.isPending.value"
        :aria-label="alert.is_active ? 'Pause alert' : 'Resume alert'"
        @update:checked="toggleActive"
      />
      <Button size="icon" variant="ghost" aria-label="Edit alert" @click="startEdit">
        <Pencil class="size-4" />
      </Button>
      <Button
        size="icon"
        variant="ghost"
        class="text-muted-foreground hover:text-destructive"
        aria-label="Delete alert"
        :disabled="remove.isPending.value"
        @click="onDelete"
      >
        <Trash2 class="size-4" />
      </Button>
    </div>
  </div>
</template>
