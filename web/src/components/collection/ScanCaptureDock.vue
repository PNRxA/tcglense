<script setup lang="ts">
import { CameraOff, Loader2, ScanLine, SwitchCamera } from '@lucide/vue'
import { Button } from '@/components/ui/button'

defineProps<{
  statusHint: string
  captureLabel: string
  captureDisabled: boolean
  controlsDisabled: boolean
  stopDisabled: boolean
  stopping: boolean
  matchName: string | null
  addedCount: number
}>()

const emit = defineEmits<{
  capture: []
  switchCamera: []
  stop: []
  review: []
}>()
</script>

<template>
  <div
    data-testid="scan-capture-dock"
    class="bg-background/90 fixed inset-x-0 bottom-0 z-30 border-y px-4 pt-2 pb-[max(0.75rem,env(safe-area-inset-bottom))] shadow-[0_-8px_24px_rgba(0,0,0,0.08)] backdrop-blur-md lg:static lg:mx-auto lg:mt-3 lg:w-full lg:max-w-md lg:border-0 lg:bg-transparent lg:px-0 lg:py-0 lg:shadow-none lg:backdrop-blur-none"
  >
    <div class="mx-auto w-full max-w-md space-y-2">
      <div class="flex min-h-5 items-center gap-2">
        <p
          class="text-muted-foreground min-w-0 flex-1 text-xs sm:text-center [overflow-wrap:anywhere]"
        >
          {{ statusHint }}
        </p>
        <Button
          v-if="matchName"
          variant="ghost"
          size="sm"
          class="min-h-11 shrink-0 px-2 lg:hidden"
          @click="emit('review')"
        >
          Review
        </Button>
        <span
          v-if="addedCount > 0"
          class="bg-muted text-muted-foreground shrink-0 rounded-full px-2 py-0.5 text-xs tabular-nums"
          :aria-label="`${addedCount} cards added this session`"
        >
          {{ addedCount }} added
        </span>
      </div>

      <div class="grid grid-cols-[3rem_minmax(0,1fr)_3rem] items-center gap-2">
        <Button
          variant="outline"
          size="icon"
          class="size-12"
          :disabled="controlsDisabled"
          aria-label="Switch camera"
          @click="emit('switchCamera')"
        >
          <SwitchCamera class="size-5" aria-hidden="true" />
        </Button>

        <Button
          class="h-12 min-w-0 px-3 text-sm sm:text-base"
          :class="{ 'pointer-events-none opacity-50': captureDisabled }"
          :aria-disabled="captureDisabled"
          @click="!captureDisabled && emit('capture')"
        >
          <ScanLine class="size-5" aria-hidden="true" />
          <span class="truncate">{{ captureLabel }}</span>
        </Button>

        <Button
          variant="outline"
          size="icon"
          class="size-12"
          :disabled="stopDisabled"
          :aria-label="stopping ? 'Saving the final card' : 'Stop scanning'"
          @click="emit('stop')"
        >
          <Loader2
            v-if="stopping"
            class="size-5 animate-spin motion-reduce:animate-none"
            aria-hidden="true"
          />
          <CameraOff v-else class="size-5" aria-hidden="true" />
        </Button>
      </div>
    </div>
  </div>
</template>
