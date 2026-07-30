<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Trash2 } from '@lucide/vue'
import { Button, buttonVariants } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import DeckPickerField from '@/components/life/DeckPickerField.vue'
import { ROTATION_OPTIONS } from '@/lib/lifeLayout'
import type { LifeRotation } from '@/lib/api/life'
import type { LifeSeat } from '@/lib/api'

// One seat's own settings, opened from its tile: rename, link a deck, turn it to face where the
// player actually sits, correct the total, or take the seat off the table.
//
// Correcting the total lives here rather than on the tile because it's rare and destructive-ish —
// and it goes through the same life endpoint as a tap, so the correction lands in the history as
// "set to 31" instead of quietly moving the number.
const props = defineProps<{
  open: boolean
  seat: LifeSeat | null
  game: string
  /** False when the seat can't be removed (it's the last one). */
  removable: boolean
  busy?: boolean
}>()

const emit = defineEmits<{
  'update:open': [value: boolean]
  save: [value: { name: string; deck_id: number | null; rotation: LifeRotation }]
  'set-life': [life: number]
  remove: []
}>()

const name = ref('')
const deckId = ref<number | null>(null)
const rotation = ref<LifeRotation>(0)
const lifeInput = ref(0)

watch(
  () => [props.open, props.seat?.id] as const,
  () => {
    if (!props.open || !props.seat) return
    name.value = props.seat.name
    deckId.value = props.seat.deck_id
    rotation.value = props.seat.rotation as LifeRotation
    lifeInput.value = props.seat.life
  },
  { immediate: true },
)

const rotationModel = computed({
  get: () => String(rotation.value),
  set: (value: string) => {
    if (value) rotation.value = Number(value) as LifeRotation
  },
})

const lifeChanged = computed(
  () => Number.isFinite(lifeInput.value) && lifeInput.value !== props.seat?.life,
)

function removeSeat() {
  emit('remove')
  emit('update:open', false)
}

function save() {
  const trimmed = name.value.trim()
  if (!trimmed) return
  // The seat write is a full replace, so send all three fields as they now stand.
  emit('save', { name: trimmed, deck_id: deckId.value, rotation: rotation.value })
  // A corrected total is a separate, recorded change — not part of the seat's metadata.
  if (lifeChanged.value) emit('set-life', lifeInput.value)
  emit('update:open', false)
}
</script>

<template>
  <Dialog :open="open" @update:open="(value: boolean) => emit('update:open', value)">
    <DialogContent
      class="bg-background max-h-[85dvh] w-[min(92vw,24rem)] overflow-y-auto rounded-xl border p-6 shadow-xl"
    >
      <DialogTitle>Seat settings</DialogTitle>
      <DialogDescription>
        Rename this player, link a deck, or turn their tile to face where they're sitting.
      </DialogDescription>

      <form class="mt-4 space-y-4" @submit.prevent="save">
        <div class="space-y-2">
          <Label for="seat-name">Name</Label>
          <Input id="seat-name" v-model="name" autofocus />
        </div>

        <div class="space-y-2">
          <Label>Deck</Label>
          <DeckPickerField v-model="deckId" :game="game" />
        </div>

        <div class="space-y-2">
          <Label>Facing</Label>
          <ToggleGroup v-model="rotationModel" type="single" variant="outline" class="flex-wrap">
            <ToggleGroupItem
              v-for="option in ROTATION_OPTIONS"
              :key="option.value"
              :value="String(option.value)"
              :aria-label="option.label"
            >
              {{ option.label }}
            </ToggleGroupItem>
          </ToggleGroup>
        </div>

        <div class="space-y-2">
          <Label for="seat-life">Life total</Label>
          <Input id="seat-life" v-model.number="lifeInput" type="number" class="w-28" />
          <p class="text-muted-foreground text-xs">
            Correcting the total records it in the history as a correction.
          </p>
        </div>

        <div class="flex items-center justify-between gap-2 pt-1">
          <Button
            v-if="removable"
            type="button"
            variant="ghost"
            class="text-destructive hover:text-destructive"
            :disabled="busy"
            @click="removeSeat"
          >
            <Trash2 class="size-4" /> Remove
          </Button>
          <span v-else />
          <div class="flex gap-2">
            <DialogClose :class="buttonVariants({ variant: 'ghost' })">Cancel</DialogClose>
            <Button type="submit" :disabled="!name.trim() || busy">Save</Button>
          </div>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>
