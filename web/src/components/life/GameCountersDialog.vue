<script setup lang="ts">
import { ref, watch } from 'vue'
import { Button, buttonVariants } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import CounterPicker from '@/components/life/CounterPicker.vue'
import type { LifeCounterKind } from '@/lib/lifeCounters'

// Change what a game in progress is tracking — a pod that starts on life alone and then plays an
// infect deck shouldn't have to start a new game to count poison.
//
// The copy states the one thing that isn't obvious: turning a counter **off** hides its row and
// keeps its values. That matters because the alternative reading — that switching off discards
// what was recorded — would make this control feel destructive, and the API deliberately doesn't
// delete history for a display choice.
const props = defineProps<{ open: boolean; counters: LifeCounterKind[]; busy?: boolean }>()
const emit = defineEmits<{
  'update:open': [value: boolean]
  save: [counters: LifeCounterKind[]]
}>()

const draft = ref<LifeCounterKind[]>([...props.counters])

// Re-seed each time it opens, so a cancelled edit doesn't linger into the next one.
watch(
  () => props.open,
  (open) => {
    if (open) draft.value = [...props.counters]
  },
)
</script>

<template>
  <Dialog :open="open" @update:open="(value: boolean) => emit('update:open', value)">
    <DialogContent
      class="bg-background max-h-[85dvh] w-[min(92vw,26rem)] overflow-y-auto rounded-xl border p-6 shadow-xl"
    >
      <DialogTitle>Counters</DialogTitle>
      <DialogDescription>
        What this game tracks besides life. Turning one off hides it — nothing already recorded is
        deleted.
      </DialogDescription>

      <div class="mt-4">
        <CounterPicker v-model="draft" id-prefix="game-counter" />
      </div>

      <div class="mt-5 flex justify-end gap-2">
        <DialogClose :class="buttonVariants({ variant: 'ghost' })">Cancel</DialogClose>
        <Button :disabled="busy" @click="emit('save', draft)">Save</Button>
      </div>
    </DialogContent>
  </Dialog>
</template>
