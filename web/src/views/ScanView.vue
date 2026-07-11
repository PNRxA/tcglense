<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { Camera, CameraOff, Loader2, ScanLine, SwitchCamera, TriangleAlert } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import CardImage from '@/components/cards/CardImage.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import QuickAddBox from '@/components/collection/QuickAddBox.vue'
import ScanMatchPanel from '@/components/collection/ScanMatchPanel.vue'
import ScanSessionList from '@/components/collection/ScanSessionList.vue'
import { useCardScanner } from '@/composables/useCardScanner'
import { useScanSession } from '@/composables/useScanSession'
import { usePageMeta } from '@/lib/seo'

// Scan physical cards into the collection with the phone/webcam camera. Tap the camera (or
// the Capture button) to take a shot: the app detects + deskews the card, identifies it
// visually (a perceptual-hash fingerprint), and offers the closest matches to pick from,
// pinning the exact printing from an OCR of the set line. Capturing the NEXT card commits
// the previous one — a deliberate bulk-add rhythm. The photo never leaves the device; only
// the small fingerprint is sent. The route is auth-gated, so this only renders when signed in.
usePageMeta({ title: 'Scan cards', canonicalPath: '/scan', noindex: true })

const game = ref('mtg')

const video = ref<HTMLVideoElement | null>(null)
const {
  status,
  errorMessage,
  ocrLoading,
  cvLoading,
  detectedQuad,
  start,
  stop,
  switchCamera,
  capture,
} = useCardScanner(video)

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
  candidates,
  log,
  addedCount,
  unrecognized,
  commitError,
  handleCapture,
  commitCurrent,
  confirmCurrent,
  discardCurrent,
  selectId,
  setName,
  adjust,
  undo,
  retryOwned,
  pickCandidate,
} = useScanSession(game)

// True while a frame is being processed — gates overlapping captures and drives the UI.
const reading = ref(false)

const isReady = computed(() => status.value === 'ready')

// The viewport tracks the camera's aspect (container matches the video, so the outline
// overlay maps 1:1 to the frame).
const videoAspect = ref(3 / 4)
function onLoadedMetadata() {
  const el = video.value
  if (el?.videoWidth && el.videoHeight) videoAspect.value = el.videoWidth / el.videoHeight
}

// Whether a card is currently detected in the live frame.
const cardDetected = computed(() => detectedQuad.value !== null)

// The detected card's outline as an SVG polygon `points` string in a 0..100 viewBox
// (normalised corners × 100), or '' when no card is detected.
const outlinePoints = computed(() =>
  detectedQuad.value
    ? detectedQuad.value.map((p) => `${(p.x * 100).toFixed(2)},${(p.y * 100).toFixed(2)}`).join(' ')
    : '',
)

// Capture + match one frame on demand — tap the camera or the Capture button. Capturing a
// new card first commits the previous one (the rapid-add rhythm), handled in the session.
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
}

async function stopScanning() {
  // Capturing the next card commits the previous — so does stopping: save the last one.
  await commitCurrent()
  discardCurrent()
  stop()
}

onBeforeUnmount(() => {
  // Best-effort save of the tentative card if the user navigates away mid-scan.
  void commitCurrent()
})

const statusHint = computed(() => {
  if (resolving.value) return 'Matching…'
  if (reading.value) return 'Scanning…'
  if (match.value) return 'Capture the next card to add this one.'
  if (cardDetected.value) return 'Card locked on — tap to scan.'
  return 'Point at a card, flat and filling the frame.'
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-8">
    <PageBreadcrumbs :items="[{ label: 'Collection', to: '/collection' }, { label: 'Scan' }]" />

    <header class="mb-6">
      <h1 class="text-3xl font-semibold tracking-tight">Scan cards</h1>
      <p class="text-muted-foreground mt-1 max-w-2xl">
        Hold a card flat and straight-on, filling the frame, then tap the camera to scan it — it's
        identified from its artwork. Pick the right match, then capture the next card to add the
        previous one.
      </p>
    </header>

    <div class="grid gap-6 lg:grid-cols-2">
      <!-- Camera + controls -->
      <section class="min-w-0">
        <div
          class="bg-muted relative mx-auto w-full max-w-md overflow-hidden rounded-xl border"
          :class="{ 'cursor-pointer': isReady && !reading }"
          :style="{ aspectRatio: String(videoAspect) }"
          role="button"
          :aria-label="isReady ? 'Tap to scan the card in view' : undefined"
          @click="isReady && captureNow()"
        >
          <!-- The live frame (always mounted so the ref/stream can attach); overlays sit on
               top per camera state. -->
          <video
            ref="video"
            class="h-full w-full object-contain"
            :class="{ 'opacity-0': !isReady }"
            autoplay
            muted
            playsinline
            @loadedmetadata="onLoadedMetadata"
          ></video>

          <!-- Live detection outline: the four corners the detector found, tracking the
               card in real time (green = locked on). Maps 1:1 to the frame (container
               matches the video aspect); non-scaling stroke stays an even width. -->
          <svg
            v-if="isReady && cardDetected"
            class="pointer-events-none absolute inset-0 h-full w-full"
            viewBox="0 0 100 100"
            preserveAspectRatio="none"
            aria-hidden="true"
          >
            <polygon
              :points="outlinePoints"
              fill="rgba(34, 197, 94, 0.12)"
              stroke="rgb(34, 197, 94)"
              stroke-width="3"
              stroke-linejoin="round"
              vector-effect="non-scaling-stroke"
            />
          </svg>

          <!-- Tap-to-scan hint (idle-ready): green + "Tap to scan" when a card is locked
               on, else a nudge to line one up. -->
          <div
            v-if="isReady && !reading && !resolving"
            class="pointer-events-none absolute bottom-2 left-1/2 -translate-x-1/2 rounded-full px-2.5 py-1 text-xs font-medium text-white"
            :class="cardDetected ? 'bg-green-600/85' : 'bg-black/60'"
          >
            {{ cardDetected ? 'Tap to scan' : 'Point at a card' }}
          </div>

          <!-- Scanning / matching pulse. -->
          <div
            v-if="isReady && (reading || resolving)"
            class="absolute top-2 left-2 flex items-center gap-1.5 rounded-full bg-black/60 px-2.5 py-1 text-xs font-medium text-white"
          >
            <Loader2 class="size-3.5 animate-spin" aria-hidden="true" />
            {{ resolving ? 'Matching' : 'Scanning' }}
          </div>

          <!-- Idle: start CTA. -->
          <div
            v-if="status === 'idle'"
            class="text-muted-foreground absolute inset-0 flex flex-col items-center justify-center gap-3 p-6 text-center"
          >
            <Camera class="size-10 opacity-60" aria-hidden="true" />
            <p class="max-w-xs text-sm">
              Camera access is needed to scan. Your photo never leaves your device — only a small
              fingerprint is sent to identify the card.
            </p>
            <Button @click.stop="startScanning">
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
            <Button v-if="status !== 'denied'" variant="outline" @click.stop="startScanning">
              Try again
            </Button>
          </div>

          <!-- Loading the detector / OCR engine (first scan of the session). -->
          <div
            v-if="isReady && (ocrLoading || cvLoading)"
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
            <Button :disabled="reading" aria-label="Capture the card in view" @click="captureNow">
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
      <section class="min-w-0 space-y-6">
        <!-- Manual fallback: type a card by name when the scan won't match it. -->
        <div>
          <label class="text-muted-foreground mb-1 block text-xs font-medium">
            Not matching? Add a card by name
          </label>
          <QuickAddBox :game="game" />
        </div>

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

        <!-- Pickable strip of the closest matches — tap the right card (its art) if the top
             pick is wrong or the match was weak. -->
        <div v-if="match && candidates.length > 1" class="rounded-xl border p-3">
          <p class="text-muted-foreground mb-2 text-xs font-medium">
            Closest matches — tap the right card
          </p>
          <div class="flex gap-2 overflow-x-auto pb-1">
            <button
              v-for="candidate in candidates"
              :key="candidate.card.id"
              type="button"
              class="group shrink-0 rounded-md focus-visible:ring-2 focus-visible:outline-none"
              :aria-label="`Pick ${candidate.card.name}`"
              :aria-pressed="candidate.card.id === selectedId"
              @click="pickCandidate(candidate.card)"
            >
              <CardImage
                :game="game"
                :id="candidate.card.id"
                :name="candidate.card.name"
                :has-image="candidate.card.has_image"
                size="small"
                class="w-20 rounded-md ring-2 transition"
                :class="
                  candidate.card.id === selectedId
                    ? 'ring-primary'
                    : 'ring-transparent group-hover:ring-muted-foreground/40'
                "
              />
            </button>
          </div>
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
            @confirm="confirmCurrent"
            @discard="discardCurrent"
          />
        </div>

        <!-- Nudge before the first match, or when the last scan didn't resolve. -->
        <div
          v-else-if="isReady"
          class="text-muted-foreground rounded-xl border border-dashed p-6 text-center text-sm"
        >
          <template v-if="unrecognized">
            Couldn't recognise that card. Hold it flat and straight-on (not tilted), close and
            filling the frame, with a contrasting background — or its set may not be in the catalog
            yet.
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
