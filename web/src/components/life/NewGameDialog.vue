<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Plus } from '@lucide/vue'
import { Button, buttonVariants } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import DeckFormatField from '@/components/decks/DeckFormatField.vue'
import SeatLinkField from '@/components/life/SeatLinkField.vue'
import LayoutPicker from '@/components/life/LayoutPicker.vue'
import { defaultLayoutFor, PLAYER_COUNT_OPTIONS, STARTING_LIFE_PRESETS } from '@/lib/lifeLayout'
import type { LifeLayout, StartLifeSessionBody } from '@/lib/api/life'
import { useLifeSetupStore } from '@/stores/lifeSetup'

// Set up a game: how many players, what they start on, how they're sitting, and who's playing
// what.
//
// The dialog opens on the settings you used last time (persisted in `stores/lifeSetup`), because
// a pod plays the same shape of game over and over — so the common case is open, confirm, play.
// Seat names and deck links are deliberately *not* remembered: reusing last game's totals is
// convenient, silently reusing last game's players would be wrong.
defineProps<{ game: string; busy?: boolean }>()
const emit = defineEmits<{ start: [body: StartLifeSessionBody] }>()

const open = ref(false)
const setup = useLifeSetupStore()

/** A seat row as the form holds it — both fields always present, unlike the wire shape's
 * optional ones, so the inputs bind without null-juggling. */
interface SeatRow {
  name: string
  deckId: number | null
  commanderCardId: string | null
}

const name = ref('')
const seats = ref<SeatRow[]>([])
/** Whether the starting life came from a preset or was typed in. */
const customLife = ref(false)

/** Reset the per-game fields each time the dialog opens, keeping the remembered shape. */
watch(open, (isOpen) => {
  if (!isOpen) return
  name.value = ''
  customLife.value = !STARTING_LIFE_PRESETS.includes(
    setup.startingLife as (typeof STARTING_LIFE_PRESETS)[number],
  )
  resizeSeats(setup.playerCount)
})

function resizeSeats(count: number) {
  const next: SeatRow[] = []
  for (let index = 0; index < count; index += 1) {
    next.push(seats.value[index] ?? { name: '', deckId: null, commanderCardId: null })
  }
  seats.value = next
}

const playerCount = computed({
  get: () => setup.playerCount,
  set: (value: number) => {
    setup.playerCount = value
    resizeSeats(value)
    // A layout that doesn't exist at the new count would render as something else; move to the
    // arrangement this count is normally played in instead.
    if (!layoutValidFor(setup.layout, value)) setup.layout = defaultLayoutFor(value)
  },
})

function layoutValidFor(layout: LifeLayout, count: number): boolean {
  if (layout === 'facing') return count >= 2
  if (layout === 'pinwheel') return count === 3 || count === 4
  return true
}

const startingLife = computed({
  get: () => setup.startingLife,
  set: (value: number) => (setup.startingLife = value),
})

/**
 * The custom box's own value, which may be empty while the user clears it to type a new number.
 * The store behind it is persisted *and* submitted, so it only follows once there's a real number
 * to follow — an empty box means "still typing", not "no starting life".
 */
const customLifeInput = ref<number | ''>(setup.startingLife)
watch(
  () => setup.startingLife,
  (value) => {
    if (Number(customLifeInput.value) !== value) customLifeInput.value = value
  },
)
watch(customLifeInput, (value) => {
  if (value !== '' && Number.isFinite(Number(value))) startingLife.value = Number(value)
})

const layout = computed({
  get: () => setup.layout,
  set: (value: LifeLayout) => (setup.layout = value),
})

// A ToggleGroup's model is a string; these bridges keep the store numeric.
const playerCountModel = computed({
  get: () => String(playerCount.value),
  set: (value: string) => {
    if (value) playerCount.value = Number(value)
  },
})
const lifePresetModel = computed({
  get: () => (customLife.value ? 'custom' : String(startingLife.value)),
  set: (value: string) => {
    if (!value) return
    if (value === 'custom') {
      customLife.value = true
      return
    }
    customLife.value = false
    startingLife.value = Number(value)
  },
})

function submit() {
  emit('start', {
    name: name.value.trim() || undefined,
    format: setup.format || undefined,
    starting_life: startingLife.value,
    layout: layout.value,
    players: seats.value.map((seat) => ({
      // A blank name is left to the server, which fills in "Player 3".
      name: seat.name.trim() || undefined,
      deck_id: seat.deckId,
      commander_card_id: seat.commanderCardId,
    })),
  })
  open.value = false
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogTrigger as-child>
      <Button :disabled="busy"><Plus class="size-4" /> New game</Button>
    </DialogTrigger>
    <DialogContent
      class="bg-background max-h-[85dvh] w-[min(92vw,32rem)] overflow-y-auto rounded-xl border p-6 shadow-xl"
    >
      <DialogTitle>New game</DialogTitle>
      <DialogDescription>
        Set the table up. Link a player to one of your decks to build its win record, or just name
        the commander they brought.
      </DialogDescription>

      <form class="mt-4 space-y-5" @submit.prevent="submit">
        <div class="space-y-2">
          <Label for="life-game-name"
            >Name <span class="text-muted-foreground">(optional)</span></Label
          >
          <Input id="life-game-name" v-model="name" placeholder="Friday pod" autofocus />
        </div>

        <div class="space-y-2">
          <Label>Players</Label>
          <ToggleGroup v-model="playerCountModel" type="single" variant="outline">
            <ToggleGroupItem
              v-for="count in PLAYER_COUNT_OPTIONS"
              :key="count"
              :value="String(count)"
              :aria-label="`${count} players`"
              class="w-10"
            >
              {{ count }}
            </ToggleGroupItem>
          </ToggleGroup>
        </div>

        <div class="space-y-2">
          <Label>Starting life</Label>
          <div class="flex flex-wrap items-center gap-2">
            <ToggleGroup v-model="lifePresetModel" type="single" variant="outline">
              <ToggleGroupItem
                v-for="preset in STARTING_LIFE_PRESETS"
                :key="preset"
                :value="String(preset)"
                :aria-label="`${preset} life`"
                class="w-12"
              >
                {{ preset }}
              </ToggleGroupItem>
              <ToggleGroupItem value="custom" aria-label="Custom starting life">
                Custom
              </ToggleGroupItem>
            </ToggleGroup>
            <Input
              v-if="customLife"
              v-model.number="customLifeInput"
              type="number"
              min="1"
              max="9999"
              class="w-24"
              aria-label="Custom starting life"
            />
          </div>
        </div>

        <div class="space-y-2">
          <Label>Format <span class="text-muted-foreground">(optional)</span></Label>
          <DeckFormatField v-model="setup.format" :game="game" />
        </div>

        <div class="space-y-2">
          <Label>Seating</Label>
          <LayoutPicker v-model="layout" :player-count="playerCount" />
        </div>

        <fieldset class="space-y-2">
          <legend class="mb-2 text-sm font-medium">Who's playing</legend>
          <div v-for="(seat, index) in seats" :key="index" class="rounded-lg border p-3">
            <Input
              v-model="seat.name"
              :placeholder="`Player ${index + 1}`"
              :aria-label="`Name for player ${index + 1}`"
            />
            <!-- Your deck, or just their commander — see SeatLinkField for why it's one control. -->
            <SeatLinkField
              class="mt-2"
              :game="game"
              :deck-id="seat.deckId"
              :commander-card-id="seat.commanderCardId"
              :seat-label="seat.name.trim() || `player ${index + 1}`"
              @update:deck-id="(value) => (seat.deckId = value)"
              @update:commander-card-id="(value) => (seat.commanderCardId = value)"
            />
          </div>
        </fieldset>

        <div class="flex justify-end gap-2">
          <DialogClose :class="buttonVariants({ variant: 'ghost' })">Cancel</DialogClose>
          <Button type="submit" :disabled="busy">Start game</Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>
