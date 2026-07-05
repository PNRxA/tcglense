<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import {
  Camera,
  CameraOff,
  Loader2,
  ScanLine,
  SwitchCamera,
  TriangleAlert,
} from '@lucide/vue'
import { Button } from '@/components/ui/button'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import ScanMatchPanel from '@/components/collection/ScanMatchPanel.vue'
import ScanSessionList from '@/components/collection/ScanSessionList.vue'
import { useCardScanner } from '@/composables/useCardScanner'
import { useScanSession } from '@/composables/useScanSession'
import { cleanCardName, isSameHeldCard, sameCardText } from '@/lib/scan/ocr'
import {
  CARD_ASPECT,
  GUIDE_MARGIN,
  NAME_REGION,
  SET_REGION,
  rectToPercentStyle,
} from '@/lib/scan/regions'
import { usePageMeta } from '@/lib/seo'
import { cn } from '@/lib/utils'

// Scan physical cards into the collection with the phone/webcam camera. The user aligns a
// card to the on-screen guide; the app OCRs its name + set line on-device, matches it
// against the catalog, and shows an editable match. Showing the NEXT card commits the
// previous one — a hands-free bulk-add rhythm — with a manual "Capture" mode as a fallback.
//
// MTG only for now: the set/collector parsing is tuned to the modern MTG frame. The route
// is auth-gated, so this view only ever renders for a signed-in user.
usePageMeta({ title: 'Scan cards', canonicalPath: '/scan', noindex: true })

const game = ref('mtg')

const video = ref<HTMLVideoElement | null>(null)
const { status, errorMessage, ocrLoading, start, stop, switchCamera, capture } =
  useCardScanner(video)

const {
  match,
  prints,
  printsLoading,
  selectedId,
  selectedCard,
  owned,
  target,
  ready,
  resolving,
  ownedError,
  log,
  addedCount,
  lastUnmatched,
  commitError,
  handleCapture,
  commitCurrent,
  discardCurrent,
  selectId,
  setName,
  adjust,
  undo,
  retryOwned,
} = useScanSession(game)

// Auto-detect a new card and commit the previous one, vs. tapping to capture each. Auto is
// hands-free (continuous OCR); manual is lighter on the device.
const autoMode = ref(true)
// True while a frame is being OCR'd — gates overlapping captures and drives the UI.
const reading = ref(false)

// Cadence of the continuous scan loop. Effective rate is max(interval, OCR time) thanks to
// the `reading` guard, so this just paces how often we try when idle.
const SCAN_INTERVAL_MS = 700
// A newly-seen name must survive this many consecutive reads before it's accepted, so a
// one-frame misread never auto-commits the wrong card.
const STABILITY_READS = 2

let loopTimer: number | null = null
let pendingText = ''
let pendingReads = 0

const isReady = computed(() => status.value === 'ready')

// The viewport tracks the camera's aspect so the CSS guide box maps 1:1 to the pixels the
// OCR crops from the frame (see regions.ts).
const videoAspect = ref(3 / 4)
function onLoadedMetadata() {
  const el = video.value
  if (el?.videoWidth && el.videoHeight) videoAspect.value = el.videoWidth / el.videoHeight
}

// The card-shaped guide box as a fraction of the (aspect-matched) container — the largest
// 61:85 rect that fits with the standard margin, mirroring guideRect(). The huge spread
// box-shadow dims everything outside the box (clipped by the viewport's overflow-hidden).
const guideStyle = computed(() => {
  const avail = 1 - 2 * GUIDE_MARGIN
  const heightLimited = videoAspect.value > CARD_ASPECT
  const widthFrac = heightLimited ? (avail * CARD_ASPECT) / videoAspect.value : avail
  const heightFrac = heightLimited ? avail : (avail * videoAspect.value) / CARD_ASPECT
  return {
    width: `${widthFrac * 100}%`,
    height: `${heightFrac * 100}%`,
    boxShadow: '0 0 0 100vmax rgba(0, 0, 0, 0.35)',
  }
})
const nameStripStyle = rectToPercentStyle(NAME_REGION)
const setStripStyle = rectToPercentStyle(SET_REGION)

function startLoop() {
  if (loopTimer !== null || !autoMode.value) return
  loopTimer = window.setInterval(() => void tick(), SCAN_INTERVAL_MS)
}
function stopLoop() {
  if (loopTimer !== null) {
    clearInterval(loopTimer)
    loopTimer = null
  }
  pendingText = ''
  pendingReads = 0
}

// One continuous-scan step: OCR the frame, and only act once a *new* card has been read
// stably. The same card still held in front of the camera is ignored (no re-commit).
async function tick() {
  if (!isReady.value || reading.value) return
  reading.value = true
  try {
    const cap = await capture()
    if (!cap) return
    const cleaned = cleanCardName(cap.nameText)
    if (cleaned.length < 3) {
      pendingText = ''
      pendingReads = 0
      return
    }
    if (match.value && isSameHeldCard(cleaned, match.value.ocrName)) {
      // The current card is still on screen — hold steady, nothing to do.
      pendingText = ''
      pendingReads = 0
      return
    }
    if (sameCardText(cleaned, pendingText)) {
      pendingReads += 1
    } else {
      pendingText = cleaned
      pendingReads = 1
    }
    if (pendingReads >= STABILITY_READS) {
      pendingText = ''
      pendingReads = 0
      await handleCapture(cap)
    }
  } finally {
    reading.value = false
  }
}

// Manual mode: OCR + resolve one frame on demand (bypasses the stability gate).
async function captureNow() {
  if (!isReady.value || reading.value) return
  reading.value = true
  try {
    const cap = await capture()
    if (cap) await handleCapture(cap)
  } finally {
    reading.value = false
  }
}

async function startScanning() {
  await start()
  if (isReady.value) startLoop()
}

async function stopScanning() {
  stopLoop()
  // Showing the next card commits the previous — so does stopping: save the last one.
  await commitCurrent()
  discardCurrent()
  stop()
}

function setAutoMode(on: boolean) {
  autoMode.value = on
  if (!isReady.value) return
  if (on) startLoop()
  else stopLoop()
}

// If the camera leaves 'ready' for any reason (stopped, switched, disconnected mid-session,
// or a failed restart), disarm the scan loop so it isn't left firing at a dead frame.
watch(status, (s) => {
  if (s !== 'ready') stopLoop()
})

onBeforeUnmount(() => {
  stopLoop()
  // Best-effort save of the tentative card if the user navigates away mid-scan.
  void commitCurrent()
})

const statusHint = computed(() => {
  if (resolving.value) return 'Matching…'
  if (reading.value) return 'Reading…'
  if (match.value) return 'Show the next card to add this one.'
  return 'Hold a card inside the frame.'
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-8">
    <PageBreadcrumbs :items="[{ label: 'Collection', to: '/collection' }, { label: 'Scan' }]" />

    <header class="mb-6">
      <h1 class="text-3xl font-semibold tracking-tight">Scan cards</h1>
      <p class="text-muted-foreground mt-1 max-w-2xl">
        Point your camera at a Magic card to add it to your collection. Show the next card and
        the previous one is added automatically — edit the match first if you need to.
      </p>
    </header>

    <div class="grid gap-6 lg:grid-cols-2">
      <!-- Camera + controls -->
      <section>
        <div
          class="bg-muted relative mx-auto w-full max-w-md overflow-hidden rounded-xl border"
          :style="{ aspectRatio: String(videoAspect) }"
        >
          <!-- The live frame (always mounted so the ref/stream can attach); overlays sit on
               top per camera state. -->
          <video
            ref="video"
            class="h-full w-full object-cover"
            :class="{ 'opacity-0': !isReady }"
            autoplay
            muted
            playsinline
            @loadedmetadata="onLoadedMetadata"
          ></video>

          <!-- Alignment guide + name/set strips, shown while the camera is live. -->
          <div
            v-if="isReady"
            class="pointer-events-none absolute inset-0 flex items-center justify-center"
          >
            <div class="relative rounded-lg border-2 border-white/70" :style="guideStyle">
              <div
                class="border-primary/80 bg-primary/10 absolute rounded-sm border"
                :style="nameStripStyle"
              ></div>
              <div class="border-primary/60 absolute rounded-sm border" :style="setStripStyle"></div>
            </div>
          </div>

          <!-- Reading / matching pulse. -->
          <div
            v-if="isReady && (reading || resolving)"
            class="absolute top-2 left-2 flex items-center gap-1.5 rounded-full bg-black/60 px-2.5 py-1 text-xs font-medium text-white"
          >
            <Loader2 class="size-3.5 animate-spin" aria-hidden="true" />
            {{ resolving ? 'Matching' : 'Reading' }}
          </div>

          <!-- Idle: start CTA. -->
          <div
            v-if="status === 'idle'"
            class="text-muted-foreground absolute inset-0 flex flex-col items-center justify-center gap-3 p-6 text-center"
          >
            <Camera class="size-10 opacity-60" aria-hidden="true" />
            <p class="max-w-xs text-sm">
              Camera access is needed to scan. Nothing is uploaded — matching runs on your device.
            </p>
            <Button @click="startScanning">
              <ScanLine class="size-4" aria-hidden="true" />
              Start scanning
            </Button>
          </div>

          <!-- Starting. -->
          <div
            v-else-if="status === 'starting'"
            class="text-muted-foreground absolute inset-0 flex items-center justify-center gap-2 text-sm"
          >
            <Loader2 class="size-4 animate-spin" aria-hidden="true" />
            Starting camera…
          </div>

          <!-- Permission denied / no camera / error. -->
          <div
            v-else-if="status === 'denied' || status === 'unavailable' || status === 'error'"
            class="text-muted-foreground absolute inset-0 flex flex-col items-center justify-center gap-3 p-6 text-center"
          >
            <CameraOff class="size-10 opacity-60" aria-hidden="true" />
            <p class="max-w-xs text-sm">{{ errorMessage }}</p>
            <Button v-if="status !== 'denied'" variant="outline" @click="startScanning">
              Try again
            </Button>
          </div>

          <!-- Loading the OCR engine (first scan of the session). -->
          <div
            v-if="isReady && ocrLoading"
            class="absolute right-2 bottom-2 flex items-center gap-1.5 rounded-full bg-black/60 px-2.5 py-1 text-xs text-white"
          >
            <Loader2 class="size-3.5 animate-spin" aria-hidden="true" />
            Preparing scanner…
          </div>
        </div>

        <!-- Controls under the viewport. -->
        <div v-if="isReady" class="mx-auto mt-3 flex w-full max-w-md flex-col gap-3">
          <p class="text-muted-foreground text-center text-sm" aria-live="polite">
            {{ statusHint }}
          </p>

          <div class="flex flex-wrap items-center justify-center gap-2">
            <!-- Auto / manual segmented toggle. -->
            <div class="bg-muted text-muted-foreground inline-flex rounded-md p-0.5 text-sm">
              <button
                type="button"
                :class="
                  cn(
                    'rounded px-3 py-1.5 font-medium transition-colors',
                    autoMode ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
                  )
                "
                @click="setAutoMode(true)"
              >
                Auto
              </button>
              <button
                type="button"
                :class="
                  cn(
                    'rounded px-3 py-1.5 font-medium transition-colors',
                    !autoMode ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
                  )
                "
                @click="setAutoMode(false)"
              >
                Manual
              </button>
            </div>

            <Button
              v-if="!autoMode"
              :disabled="reading"
              aria-label="Capture the card in view"
              @click="captureNow"
            >
              <ScanLine class="size-4" aria-hidden="true" />
              Capture
            </Button>

            <Button variant="outline" size="icon" aria-label="Switch camera" @click="switchCamera">
              <SwitchCamera class="size-4" aria-hidden="true" />
            </Button>

            <Button variant="outline" @click="stopScanning">
              <CameraOff class="size-4" aria-hidden="true" />
              Stop
            </Button>
          </div>
        </div>
      </section>

      <!-- Current match + session tally -->
      <section class="space-y-6">
        <div
          v-if="commitError"
          class="border-destructive/40 text-destructive flex items-center gap-2 rounded-lg border px-3 py-2 text-sm"
        >
          <TriangleAlert class="size-4 shrink-0" aria-hidden="true" />
          Couldn't save the last change. Check your connection and try again.
        </div>

        <div
          v-if="ownedError"
          class="border-destructive/40 text-destructive flex flex-wrap items-center gap-2 rounded-lg border px-3 py-2 text-sm"
        >
          <TriangleAlert class="size-4 shrink-0" aria-hidden="true" />
          <span>Couldn't read your current count for this printing.</span>
          <Button variant="outline" size="sm" class="ml-auto" @click="retryOwned">Retry</Button>
        </div>

        <!-- The card being edited before it commits. -->
        <div v-if="match" class="rounded-xl border p-4">
          <ScanMatchPanel
            :game="game"
            :match="match"
            :prints="prints"
            :prints-loading="printsLoading"
            :selected-card="selectedCard"
            :selected-id="selectedId"
            :owned="owned"
            :target="target"
            :ready="ready"
            :resolving="resolving"
            @name="setName"
            @select="selectId"
            @adjust="adjust"
            @discard="discardCurrent"
          />
        </div>

        <!-- Nudge before the first match, or when the last OCR didn't resolve. -->
        <div
          v-else-if="isReady"
          class="text-muted-foreground rounded-xl border border-dashed p-6 text-center text-sm"
        >
          <template v-if="lastUnmatched">
            Read “{{ lastUnmatched }}” but found no matching card in the catalog. Check the spelling,
            or that this card's set has been added yet.
          </template>
          <template v-else> The card you scan will appear here to review and edit. </template>
        </div>

        <!-- Session tally. -->
        <div v-if="addedCount > 0">
          <h2 class="mb-1 text-sm font-medium">
            Added this session
            <span class="text-muted-foreground tabular-nums">({{ addedCount }})</span>
          </h2>
          <ScanSessionList :game="game" :entries="log" @undo="undo" />
        </div>
      </section>
    </div>
  </div>
</template>
