<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Plus } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  PLAY_DEFAULT_LIFE,
  PLAY_FORMATS,
  PLAY_MAX_PLAYERS,
  PLAY_MIN_PLAYERS,
  type CreatePlayRoomBody,
  type PlayFormat,
} from '@/lib/api/play'

// Opening a table: four fields, three of which already have the right answer.
//
// Starting life follows the format until you touch it — picking Commander and then being handed
// 20 life would be wrong, and being made to type 40 every time would be tedious — so the field
// tracks the format's default and stops tracking the moment it's edited. The label is optional
// because "Thursday pod" is nice to have and nothing depends on it; the player count is a hard
// 2–6 the server also enforces.
const props = defineProps<{ busy?: boolean; error?: string | null }>()

const emit = defineEmits<{ create: [body: CreatePlayRoomBody] }>()

const label = ref('')
const format = ref<PlayFormat>('commander')
const players = ref(4)
const life = ref(PLAY_DEFAULT_LIFE.commander)
/** Set once the field is edited by hand — after that the format stops overwriting it. */
const lifeTouched = ref(false)

watch(format, (next) => {
  if (!lifeTouched.value) life.value = PLAY_DEFAULT_LIFE[next]
})

const defaultLife = computed(() => PLAY_DEFAULT_LIFE[format.value])
const playerOptions = computed(() =>
  Array.from({ length: PLAY_MAX_PLAYERS - PLAY_MIN_PLAYERS + 1 }, (_, i) => PLAY_MIN_PLAYERS + i),
)
const lifeValid = computed(
  () => Number.isInteger(life.value) && life.value >= 1 && life.value <= 999,
)

const formatSelection = computed({
  get: () => format.value,
  set: (value: string) => {
    format.value = value as PlayFormat
  },
})
const playerSelection = computed({
  get: () => String(players.value),
  set: (value: string) => {
    players.value = Number(value)
  },
})

function submit() {
  if (props.busy || !lifeValid.value) return
  emit('create', {
    label: label.value.trim() || undefined,
    format: format.value,
    starting_life: life.value,
    max_players: players.value,
  })
}
</script>

<template>
  <form class="bg-card space-y-4 rounded-xl border p-4" @submit.prevent="submit">
    <div>
      <h2 class="font-medium">Create a room</h2>
      <p class="text-muted-foreground mt-1 text-sm">
        You'll get an invite link to send your table.
      </p>
    </div>

    <div class="space-y-2">
      <Label for="play-room-label"
        >Name <span class="text-muted-foreground">(optional)</span></Label
      >
      <Input id="play-room-label" v-model="label" maxlength="60" placeholder="Thursday pod" />
    </div>

    <div class="grid gap-4 sm:grid-cols-3">
      <div class="space-y-2">
        <Label for="play-room-format">Format</Label>
        <Select v-model="formatSelection">
          <SelectTrigger id="play-room-format" class="w-full" aria-label="Format">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="entry in PLAY_FORMATS"
              :key="entry"
              :value="entry"
              class="capitalize"
            >
              {{ entry }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div class="space-y-2">
        <Label for="play-room-life">Starting life</Label>
        <Input
          id="play-room-life"
          v-model.number="life"
          type="number"
          min="1"
          max="999"
          :aria-invalid="!lifeValid"
          @input="lifeTouched = true"
        />
        <p class="text-muted-foreground text-xs">
          {{ life === defaultLife ? `Default for ${format}` : `Default is ${defaultLife}` }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="play-room-players">Players</Label>
        <Select v-model="playerSelection">
          <SelectTrigger id="play-room-players" class="w-full" aria-label="Players">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="count in playerOptions" :key="count" :value="String(count)">
              {{ count }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>
    </div>

    <Button type="submit" :disabled="busy || !lifeValid">
      <Plus class="size-4" aria-hidden="true" /> Create room
    </Button>

    <p v-if="error" class="text-destructive text-sm">{{ error }}</p>
  </form>
</template>
