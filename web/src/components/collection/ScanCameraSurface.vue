<script setup lang="ts">
import { computed, type ComponentPublicInstance } from 'vue'
import { Camera, CameraOff, Loader2, ScanLine } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import type { CameraStatus } from '@/composables/useCardScanner'
import type { Quad } from '@/lib/scan/detect'
import { guideRect } from '@/lib/scan/regions'

const props = defineProps<{
  status: CameraStatus
  errorMessage: string | null
  ocrLoading: boolean
  cvLoading: boolean
  detectedQuad: Quad | null
  captureEnabled: boolean
  captureLabel: string
  processing: boolean
  activityLabel: string
  statusHint: string
}>()

const emit = defineEmits<{
  start: []
  capture: []
}>()

const video = defineModel<HTMLVideoElement | null>('video', { required: true })
const videoAspect = defineModel<number>('aspect', { required: true })

const isReady = computed(() => props.status === 'ready')

// Round the lock-on outline's corners so it reads like a real card instead of a hard
// rectangle. A 63 mm card has a ~3 mm corner radius (≈4.8% of its width), so trimming
// ~6% of each corner's shorter edge and curving through the vertex keeps the rounding
// proportional to the detected quad — the same card-shaped feel CardImage's frame uses.
// Purely visual: the detected geometry (props.detectedQuad) is untouched.
const OUTLINE_CORNER_RATIO = 0.06
const outlinePath = computed(() => {
  const quad = props.detectedQuad
  if (!quad) return ''
  const points = quad.map((point) => ({ x: point.x * 100, y: point.y * 100 }))
  const n = points.length
  const fmt = (value: number) => value.toFixed(2)
  let path = ''
  let started = false
  for (let i = 0; i < n; i++) {
    const corner = points[i]
    const prev = points[(i - 1 + n) % n]
    const next = points[(i + 1) % n]
    if (!corner || !prev || !next) continue
    const toPrev = { x: prev.x - corner.x, y: prev.y - corner.y }
    const toNext = { x: next.x - corner.x, y: next.y - corner.y }
    const lenPrev = Math.hypot(toPrev.x, toPrev.y)
    const lenNext = Math.hypot(toNext.x, toNext.y)
    // Trim the same distance down both edges so the corner is a symmetric arc; capped by
    // the shorter edge it can never overrun a neighbouring corner (2 × 6% < 100%).
    const radius = OUTLINE_CORNER_RATIO * Math.min(lenPrev, lenNext)
    const enter =
      lenPrev > 0
        ? {
            x: corner.x + (toPrev.x / lenPrev) * radius,
            y: corner.y + (toPrev.y / lenPrev) * radius,
          }
        : corner
    const exit =
      lenNext > 0
        ? {
            x: corner.x + (toNext.x / lenNext) * radius,
            y: corner.y + (toNext.y / lenNext) * radius,
          }
        : corner
    path += `${started ? 'L' : 'M'} ${fmt(enter.x)} ${fmt(enter.y)} Q ${fmt(corner.x)} ${fmt(corner.y)} ${fmt(exit.x)} ${fmt(exit.y)} `
    started = true
  }
  return started ? `${path}Z` : ''
})
const guideStyle = computed(() => {
  const aspect = videoAspect.value
  const rect = guideRect(aspect * 100, 100)
  return {
    left: `${rect.left / aspect}%`,
    top: `${rect.top}%`,
    width: `${rect.width / aspect}%`,
    height: `${rect.height}%`,
  }
})
const surfaceStyle = computed<Record<string, string>>(() => {
  const aspect = videoAspect.value
  const widthForHeightBudget = (rem: number) =>
    `calc(${(aspect * 100).toFixed(4)}dvh - ${(aspect * rem).toFixed(4)}rem)`
  return {
    aspectRatio: String(aspect),
    '--scan-mobile-width': widthForHeightBudget(15.5),
    '--scan-short-idle-width': widthForHeightBudget(8),
    '--scan-short-active-width': widthForHeightBudget(14),
  }
})
const overlayHint = computed(() => {
  if (props.processing) return props.activityLabel
  if ((props.ocrLoading || props.cvLoading) && !props.detectedQuad) return 'Preparing scanner…'
  return props.statusHint
})

function setVideoElement(element: Element | ComponentPublicInstance | null) {
  video.value = element instanceof HTMLVideoElement ? element : null
}

function syncVideoAspect() {
  const element = video.value
  if (element?.videoWidth && element.videoHeight) {
    videoAspect.value = element.videoWidth / element.videoHeight
  }
}
</script>

<template>
  <div
    data-testid="scan-camera"
    class="scan-camera-surface bg-muted relative mx-auto max-w-full overflow-hidden rounded-xl border"
    :class="{ 'scan-camera-surface--active': isReady }"
    :style="surfaceStyle"
    :aria-busy="processing || undefined"
  >
    <!-- Keep the video mounted while the camera state changes so its stream/ref stays intact. -->
    <video
      :ref="setVideoElement"
      class="h-full w-full object-contain"
      :class="{ 'opacity-0': !isReady }"
      autoplay
      muted
      playsinline
      @loadedmetadata="syncVideoAspect"
      @resize="syncVideoAspect"
    ></video>

    <!-- A quiet always-on target teaches the required edge spacing before detection locks. -->
    <div
      v-if="isReady && !detectedQuad"
      data-testid="scan-guide"
      class="pointer-events-none absolute rounded-lg border-2 border-dashed border-white/80 shadow-[0_0_0_2px_rgba(0,0,0,0.8),0_0_0_9999px_rgba(0,0,0,0.08)]"
      :style="guideStyle"
      aria-hidden="true"
    ></div>

    <svg
      v-if="isReady && detectedQuad"
      data-testid="scan-outline"
      class="pointer-events-none absolute inset-0 h-full w-full"
      viewBox="0 0 100 100"
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <path
        :d="outlinePath"
        fill="none"
        stroke="rgba(0, 0, 0, 0.8)"
        stroke-width="7"
        stroke-linejoin="round"
        vector-effect="non-scaling-stroke"
      />
      <path
        :d="outlinePath"
        fill="rgba(34, 197, 94, 0.12)"
        stroke="rgb(34, 197, 94)"
        stroke-width="3"
        stroke-linejoin="round"
        vector-effect="non-scaling-stroke"
      />
    </svg>

    <!-- Keep this shortcut mounted while the camera is live so a keyboard capture never drops
         focus while processing temporarily disables the action. -->
    <button
      v-if="isReady"
      type="button"
      class="absolute inset-0 z-10 cursor-pointer focus-visible:ring-3 focus-visible:ring-inset focus-visible:ring-ring/60 focus-visible:outline-none"
      :class="{ 'cursor-default': !captureEnabled }"
      :aria-disabled="!captureEnabled"
      :aria-label="`Camera preview — ${captureLabel}`"
      @click="captureEnabled && emit('capture')"
    ></button>

    <div
      v-if="isReady"
      class="pointer-events-none absolute right-3 bottom-3 left-3 z-20 flex justify-center"
      aria-hidden="true"
    >
      <div
        class="flex max-w-full items-center gap-1.5 rounded-full bg-black/65 px-3 py-1.5 text-center text-xs font-medium text-white backdrop-blur-sm"
      >
        <Loader2
          v-if="processing || ocrLoading || cvLoading"
          class="size-3.5 shrink-0 animate-spin motion-reduce:animate-none"
        />
        <span class="line-clamp-2">{{ overlayHint }}</span>
      </div>
    </div>

    <div
      v-if="status === 'idle'"
      class="text-muted-foreground absolute inset-0 flex flex-col items-center justify-center gap-3 p-6 text-center"
    >
      <Camera class="scan-idle-icon size-10 opacity-60" aria-hidden="true" />
      <p class="scan-idle-copy max-w-xs text-sm">
        Camera access is needed to scan. Your photo never leaves your device — only a small
        fingerprint is sent to identify the card.
      </p>
      <Button class="min-h-11" @click="emit('start')">
        <ScanLine class="size-4" aria-hidden="true" />
        Start scanning
      </Button>
    </div>

    <div
      v-else-if="status === 'starting'"
      class="text-muted-foreground absolute inset-0 flex items-center justify-center gap-2 text-sm"
      role="status"
    >
      <Loader2 class="size-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
      Starting camera…
    </div>

    <div
      v-else-if="status === 'denied' || status === 'unavailable' || status === 'error'"
      class="text-muted-foreground absolute inset-0 flex flex-col items-center justify-center gap-3 p-6 text-center"
      role="alert"
    >
      <CameraOff class="size-10 opacity-60" aria-hidden="true" />
      <p class="max-w-xs text-sm">{{ errorMessage }}</p>
      <Button v-if="status !== 'denied'" variant="outline" class="min-h-11" @click="emit('start')">
        Try again
      </Button>
    </div>
  </div>
</template>

<style scoped>
/* The camera, guide, and polygon must share one exact aspect-ratio box. Compute a width that
   fits both the page and the available viewport height, then let aspect-ratio derive height. */
.scan-camera-surface {
  width: min(100%, var(--scan-mobile-width));
  height: auto;
}

@media (min-width: 40rem) and (max-width: 63.999rem) {
  .scan-camera-surface {
    width: min(100%, 28rem, var(--scan-mobile-width));
  }
}

@media (min-width: 64rem) {
  .scan-camera-surface {
    width: 100%;
    max-width: 28rem;
  }
}

/* Keep setup and active controls visible without distorting the frame on a rotated phone. */
@media (orientation: landscape) and (max-height: 31.25rem) {
  .scan-camera-surface {
    width: min(100%, var(--scan-short-idle-width));
    max-width: 100%;
  }

  .scan-camera-surface--active {
    width: min(100%, var(--scan-short-active-width));
  }
}

@media (orientation: landscape) and (max-width: 39.999rem) and (max-height: 31.25rem) {
  .scan-idle-icon,
  .scan-idle-copy {
    display: none;
  }
}
</style>
