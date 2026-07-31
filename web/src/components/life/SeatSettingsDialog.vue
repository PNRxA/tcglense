<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Trash2 } from '@lucide/vue'
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
import SeatLinkField from '@/components/life/SeatLinkField.vue'
import { ROTATION_OPTIONS } from '@/lib/lifeLayout'
import type { LifeRotation } from '@/lib/api/life'
import type { LifeSeat } from '@/lib/api'

// One seat's own settings, opened from its tile: rename, say what they're playing (one of your
// decks, or just their commander), move them a place around the table, turn the tile to face where
// they actually sit, correct the total, or take the seat off.
//
// Seat order and Facing are the two halves of "where does everyone sit": Facing turns one tile in
// place, seat order swaps which spot of the layout it occupies.
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
  /** How many seats the game has, so the reorder controls know their bounds. */
  seatCount: number
  busy?: boolean
}>()

const emit = defineEmits<{
  'update:open': [value: boolean]
  save: [
    value: {
      name: string
      deck_id: number | null
      commander_card_id: string | null
      rotation: LifeRotation
      /**
       * A corrected total, when the user changed it. It rides the same event as the metadata so
       * the two writes can be ordered: they touch the same seat row from different endpoints, and
       * firing them concurrently lets whichever answers last put a stale field back on screen.
       */
      life?: number
    },
  ]
  /** Shift this seat one place earlier or later in the layout's seat order. */
  move: [direction: -1 | 1]
  remove: []
}>()

const name = ref('')
const deckId = ref<number | null>(null)
const commanderCardId = ref<string | null>(null)
const rotation = ref<LifeRotation>(0)
const lifeInput = ref(0)

watch(
  () => [props.open, props.seat?.id] as const,
  () => {
    if (!props.open || !props.seat) return
    name.value = props.seat.name
    deckId.value = props.seat.deck_id
    commanderCardId.value = props.seat.commander_card_id
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

/** Each rotation option's arrow, by the screen edge it points at — see `ROTATION_OPTIONS`. */
const ROTATION_ARROWS = {
  up: ArrowUp,
  down: ArrowDown,
  left: ArrowLeft,
  right: ArrowRight,
}

// A one-seat table has no order to change.
const canReorder = computed(() => props.seatCount > 1)
const isFirst = computed(() => (props.seat?.position ?? 0) <= 0)
const isLast = computed(() => (props.seat?.position ?? 0) >= props.seatCount - 1)

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
  // The seat write is a full replace, so send every field as it now stands. A corrected total is
  // still a separate, recorded change — it just travels together so the caller can order them.
  emit('save', {
    name: trimmed,
    deck_id: deckId.value,
    commander_card_id: commanderCardId.value,
    rotation: rotation.value,
    ...(lifeChanged.value ? { life: lifeInput.value } : {}),
  })
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
        Rename this player, say what they're playing, move them around the table, or turn their tile
        to face where they're sitting.
      </DialogDescription>

      <form class="mt-4 space-y-4" @submit.prevent="save">
        <div class="space-y-2">
          <Label for="seat-name">Name</Label>
          <Input id="seat-name" v-model="name" autofocus />
        </div>

        <div class="space-y-2">
          <Label>Playing</Label>
          <SeatLinkField
            :game="game"
            :deck-id="deckId"
            :commander-card-id="commanderCardId"
            :commander-name="seat?.commander_name"
            @update:deck-id="(value) => (deckId = value)"
            @update:commander-card-id="(value) => (commanderCardId = value)"
          />
        </div>

        <!-- The other half of "where does everyone sit": Facing turns one tile, this swaps which
             spot of the layout the seat occupies. -->
        <div v-if="canReorder" class="space-y-2">
          <Label>Seat order</Label>
          <div class="flex items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              :disabled="isFirst || busy"
              @click="emit('move', -1)"
            >
              <ArrowLeft class="size-4" /> Move earlier
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              :disabled="isLast || busy"
              @click="emit('move', 1)"
            >
              Move later <ArrowRight class="size-4" />
            </Button>
          </div>
          <p class="text-muted-foreground text-xs">
            Seat {{ (seat?.position ?? 0) + 1 }} of {{ seatCount }} in the current layout.
          </p>
        </div>

        <div class="space-y-2">
          <Label>Facing</Label>
          <!-- Arrows, not words: the choice is "which side of the table are they on", and a
               direction is quicker to point at than "Left edge" is to read. -->
          <ToggleGroup v-model="rotationModel" type="single" variant="outline" class="flex-wrap">
            <ToggleGroupItem
              v-for="option in ROTATION_OPTIONS"
              :key="option.value"
              :value="String(option.value)"
              :aria-label="option.label"
              :title="option.label"
              class="w-10"
            >
              <component :is="ROTATION_ARROWS[option.arrow]" class="size-4" />
            </ToggleGroupItem>
          </ToggleGroup>
          <p class="text-muted-foreground text-xs">
            Which side of the table this player is on — the tile turns to read upright for them.
          </p>
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
