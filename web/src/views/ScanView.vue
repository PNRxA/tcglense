<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { onBeforeRouteLeave } from 'vue-router'
import { TriangleAlert } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import CardImage from '@/components/cards/CardImage.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import QuickAddBox from '@/components/collection/QuickAddBox.vue'
import ScanCameraSurface from '@/components/collection/ScanCameraSurface.vue'
import ScanCaptureDock from '@/components/collection/ScanCaptureDock.vue'
import ScanMatchPanel from '@/components/collection/ScanMatchPanel.vue'
import ScanSessionList from '@/components/collection/ScanSessionList.vue'
import { useCardScanner } from '@/composables/useCardScanner'
import { useScanSession } from '@/composables/useScanSession'
import { printingMetadataLabel } from '@/lib/printings'
import { usePageMeta } from '@/lib/seo'
import { useScanPreferencesStore } from '@/stores/scanPreferences'

// Scan physical cards into the collection with the phone/webcam camera. A capture identifies
// the card visually and uses OCR to pin its printing. The current match remains tentative until
// it is confirmed, scanning advances to another card, or the session ends.
usePageMeta({ title: 'Scan cards', canonicalPath: '/scan', noindex: true })

const scanPrefs = useScanPreferencesStore()
const game = ref('mtg')
const video = ref<HTMLVideoElement | null>(null)
const videoAspect = ref(3 / 4)
const {
  status,
  errorMessage,
  ocrLoading,
  cvStatus,
  detectedQuad,
  start,
  stop,
  switchCamera,
  capture,
} = useCardScanner(video)

const {
  match,
  prints,
  printsFilter,
  printsFiltered,
  printsLoading,
  printsLoadingMore,
  printsError,
  printsTotal,
  printsHasMore,
  selectedId,
  selectedCard,
  owned,
  target,
  ready,
  advanceReady,
  resolving,
  finalizing,
  undoing,
  ownedError,
  candidates,
  log,
  addedCount,
  unrecognized,
  commitError,
  handleCapture,
  finalizeCurrent,
  confirmCurrent,
  discardCurrent,
  selectId,
  setName,
  adjust,
  undo,
  retryOwned,
  retryPrintings,
  pickCandidate,
  loadMorePrintings,
} = useScanSession(game)

const reading = ref(false)
const stopping = ref(false)
const captureRejected = ref(false)
const isReady = computed(() => status.value === 'ready')
const cardDetected = computed(() => detectedQuad.value !== null)
const captureEnabled = computed(
  () =>
    isReady.value &&
    cardDetected.value &&
    advanceReady.value &&
    !reading.value &&
    !resolving.value &&
    !finalizing.value &&
    !undoing.value &&
    !stopping.value,
)

watch([status, cardDetected], ([nextStatus, hasCard]) => {
  if (captureRejected.value && (nextStatus !== 'ready' || hasCard)) {
    captureRejected.value = false
  }
})

const successMessage = ref<string | null>(null)
let successTimer: number | null = null
watch(addedCount, (count, previous) => {
  if (count < previous) {
    if (successTimer !== null) window.clearTimeout(successTimer)
    successTimer = null
    successMessage.value = null
    return
  }
  if (count === previous) return
  const name = log.value[0]?.card.name ?? 'Card'
  successMessage.value = `Added ${name}. ${count} ${count === 1 ? 'card' : 'cards'} added this session.`
  if (successTimer !== null) window.clearTimeout(successTimer)
  successTimer = window.setTimeout(() => {
    successMessage.value = null
    successTimer = null
  }, 2500)
})

async function captureNow() {
  if (!captureEnabled.value) return
  reading.value = true
  captureRejected.value = false
  try {
    const captured = await capture()
    if (!captured) {
      captureRejected.value = true
      return
    }
    // Only a fresh card ('matched') swaps in a new panel to review; a re-scan of the same
    // card, an unrecognised frame, or a busy loop leaves the review section unchanged, so
    // there's nothing new to scroll to.
    const outcome = await handleCapture(captured)
    if (outcome === 'matched' && shouldAutoScrollAfterMatch()) reviewMatch({ focus: false })
  } finally {
    reading.value = false
  }
}

async function stopScanning() {
  // Stopping follows the same loss-prevention contract as navigation: wait for any printing
  // resolution and save the final tentative card before releasing the camera.
  if (stopping.value || finalizing.value || reading.value || resolving.value) return
  stopping.value = true
  try {
    if (!(await finalizeCurrent())) return
    discardCurrent()
    stop()
  } finally {
    stopping.value = false
  }
}

onBeforeUnmount(() => {
  if (successTimer !== null) window.clearTimeout(successTimer)
  void finalizeCurrent()
})

onBeforeRouteLeave(async () => {
  if (reading.value || resolving.value) return false
  if (!(await finalizeCurrent())) return false
  discardCurrent()
  return true
})

const activityLabel = computed(() => {
  if (finalizing.value || stopping.value) return 'Saving the final card…'
  if (undoing.value) return 'Updating the session…'
  if (resolving.value) return 'Matching the card…'
  return 'Scanning the card…'
})
const processing = computed(
  () => reading.value || resolving.value || finalizing.value || undoing.value || stopping.value,
)
const statusHint = computed(() => {
  if (processing.value) return activityLabel.value
  if (captureRejected.value) {
    return 'Keep the whole card inside the guide and hold it still, then try again.'
  }
  if (commitError.value) return "Couldn't save the last card. Check your connection and retry."
  if (ownedError.value) return "Couldn't load the count for this printing."
  if (unrecognized.value) return "That card wasn't recognised. Try again or add it by name."
  if (successMessage.value) return successMessage.value
  if (match.value && !advanceReady.value) return 'Finishing the current match…'
  if (match.value) return `Review ${match.value.name}, or add it and scan the next card.`
  if (cardDetected.value) return 'Card locked on — ready to scan.'
  return 'Fit one flat card inside the guide with space around every edge.'
})
const captureLabel = computed(() => {
  if (match.value && !advanceReady.value) return 'Finishing match…'
  return match.value ? 'Add & scan next' : 'Scan card'
})

const reviewSection = ref<HTMLElement | null>(null)
// `focus` moves keyboard focus onto the results — right for the explicit "Review" tap
// (deliberate navigation), but the automatic post-scan scroll only brings the panel into
// view: stealing focus each scan would break the rapid "Add & scan next" rhythm, and the
// aria-live region already announces the match to screen readers.
function reviewMatch({ focus = true }: { focus?: boolean } = {}) {
  const section = reviewSection.value
  if (!section) return
  const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
  section.scrollIntoView({ behavior: reduceMotion ? 'auto' : 'smooth', block: 'start' })
  if (focus) section.focus({ preventScroll: true })
}

// Auto-scroll only helps the single-column (mobile/tablet) layout, where the review section
// sits below the camera and off-screen. On the two-column (lg+) layout it's already beside
// the camera — and the toggle is hidden there — so a scroll would just yank the page around.
function shouldAutoScrollAfterMatch() {
  if (!scanPrefs.autoScrollToReview) return false
  return !window.matchMedia('(min-width: 1024px)').matches
}
</script>

<template>
  <div
    class="mx-auto max-w-6xl px-4 pt-4 sm:pt-6 lg:pt-8"
    :class="isReady ? 'pb-32 lg:pb-8' : 'pb-4 sm:pb-6 lg:pb-8'"
  >
    <div class="hidden lg:block">
      <PageBreadcrumbs :items="[{ label: 'Collection', to: '/collection' }, { label: 'Scan' }]" />
    </div>

    <header class="scan-page-header mb-3 lg:mb-6">
      <div class="flex items-center gap-2">
        <h1 class="text-2xl font-semibold tracking-tight sm:text-3xl">Scan cards</h1>
        <span
          v-if="addedCount > 0"
          class="bg-muted text-muted-foreground rounded-full px-2 py-0.5 text-xs tabular-nums lg:hidden"
        >
          {{ addedCount }} added
        </span>
      </div>
      <p
        class="text-muted-foreground mt-1 max-w-2xl text-sm sm:text-base"
        :class="{ 'hidden lg:block': status !== 'idle' }"
      >
        Fit one card inside the guide and tap Scan. Review the match; scanning the next card adds
        the previous one.
      </p>
    </header>

    <!-- Auto-scroll toggle: only the single-column layout scrolls the review into view after a
         scan, so the control lives here and is hidden on the two-column (lg+) layout where the
         review is always beside the camera. -->
    <div class="scan-auto-scroll-row mb-3 flex items-center gap-2 lg:mb-6 lg:hidden">
      <Switch
        id="scan-auto-scroll"
        :checked="scanPrefs.autoScrollToReview"
        aria-label="Auto-scroll to the review section after each successful scan"
        @update:checked="scanPrefs.setAutoScrollToReview"
      />
      <label for="scan-auto-scroll" class="cursor-pointer text-sm select-none">
        Auto-scroll to review
      </label>
    </div>

    <p class="sr-only" aria-live="polite">
      {{ isReady || successMessage ? statusHint : '' }}
    </p>

    <div class="grid gap-0 lg:grid-cols-2 lg:grid-rows-[auto_1fr] lg:gap-x-6">
      <section class="min-w-0 lg:col-start-1 lg:row-start-1">
        <ScanCameraSurface
          v-model:video="video"
          v-model:aspect="videoAspect"
          :status="status"
          :error-message="errorMessage"
          :ocr-loading="ocrLoading"
          :cv-status="cvStatus"
          :detected-quad="detectedQuad"
          :capture-enabled="captureEnabled"
          :capture-label="captureLabel"
          :processing="processing"
          :activity-label="activityLabel"
          :status-hint="statusHint"
          @start="start"
          @capture="captureNow"
        />
      </section>

      <ScanCaptureDock
        v-if="isReady"
        class="lg:col-start-1 lg:row-start-2"
        :status-hint="statusHint"
        :capture-label="captureLabel"
        :capture-disabled="!captureEnabled"
        :controls-disabled="reading || resolving || finalizing || undoing || stopping"
        :stop-disabled="finalizing || reading || resolving || stopping"
        :stopping="stopping"
        :match-name="match?.name ?? null"
        :added-count="addedCount"
        @capture="captureNow"
        @switch-camera="switchCamera"
        @stop="stopScanning"
        @review="reviewMatch"
      />

      <section
        ref="reviewSection"
        class="min-w-0 space-y-4 pt-5 focus:outline-none lg:col-start-2 lg:row-span-2 lg:row-start-1 lg:space-y-6 lg:pt-0"
        tabindex="-1"
        aria-label="Scan results"
      >
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
          <Button variant="outline" class="ml-auto min-h-11 lg:min-h-9" @click="retryOwned">
            Retry
          </Button>
        </div>

        <div
          v-if="unrecognized"
          class="border-amber-500/40 bg-amber-500/5 flex items-start gap-2 rounded-lg border px-3 py-2 text-sm"
        >
          <TriangleAlert
            class="mt-0.5 size-4 shrink-0 text-amber-600 dark:text-amber-400"
            aria-hidden="true"
          />
          <span>
            That card wasn't recognised. Keep it flat, fill the guide, and use a contrasting
            background — or add it by name below.
          </span>
        </div>

        <!-- Put the result before fallback tools so the card just scanned is the next thing read. -->
        <div v-if="match" class="rounded-xl border p-3 sm:p-4">
          <ScanMatchPanel
            v-model:filter="printsFilter"
            :game="game"
            :match="match"
            :prints="prints"
            :prints-filtered="printsFiltered"
            :prints-loading="printsLoading"
            :prints-loading-more="printsLoadingMore"
            :prints-error="printsError"
            :prints-total="printsTotal"
            :prints-has-more="printsHasMore"
            :selected-card="selectedCard"
            :selected-id="selectedId"
            :owned="owned"
            :target="target"
            :ready="ready"
            :resolving="resolving"
            :candidates="candidates"
            :disabled="finalizing || resolving || undoing"
            @name="setName"
            @select="selectId"
            @adjust="adjust"
            @confirm="confirmCurrent"
            @discard="discardCurrent"
            @load-more="loadMorePrintings"
            @retry-printings="retryPrintings"
          />
        </div>

        <div v-if="match && candidates.length > 1" class="rounded-xl border p-3">
          <p class="text-muted-foreground mb-2 text-xs font-medium">
            Closest visual matches — choose the right artwork
          </p>
          <div class="flex gap-2 overflow-x-auto pb-1">
            <button
              v-for="candidate in candidates"
              :key="candidate.card.id"
              type="button"
              class="group w-24 shrink-0 rounded-md text-left focus-visible:ring-2 focus-visible:outline-none"
              :aria-label="`Pick ${candidate.card.name}, ${printingMetadataLabel(candidate.card)}`"
              :aria-pressed="candidate.card.id === selectedId"
              :disabled="finalizing || resolving || undoing"
              @click="pickCandidate(candidate.card)"
            >
              <CardImage
                :game="game"
                :id="candidate.card.id"
                :name="candidate.card.name"
                :has-image="candidate.card.has_image"
                size="small"
                class="w-full rounded-md ring-2 transition"
                :class="
                  candidate.card.id === selectedId
                    ? 'ring-primary'
                    : 'ring-transparent group-hover:ring-muted-foreground/40'
                "
              />
              <span class="mt-1 block truncate text-xs font-medium">{{ candidate.card.name }}</span>
              <span class="text-muted-foreground block truncate text-[0.6875rem]">
                {{ printingMetadataLabel(candidate.card) }}
              </span>
            </button>
          </div>
        </div>

        <div
          v-else-if="isReady && !match"
          class="text-muted-foreground rounded-xl border border-dashed p-5 text-center text-sm"
        >
          The card you scan will appear here to review before it is added.
        </div>

        <div v-if="addedCount > 0">
          <h2 class="mb-1 text-sm font-medium">
            Added this session
            <span class="text-muted-foreground tabular-nums">({{ addedCount }})</span>
          </h2>
          <ScanSessionList
            :game="game"
            :entries="log"
            :disabled="finalizing || resolving || undoing"
            @undo="undo"
          />
        </div>

        <details class="group rounded-xl border" :open="unrecognized || undefined">
          <summary
            class="hover:bg-muted/50 flex min-h-11 cursor-pointer list-none items-center px-3 text-sm font-medium [&::-webkit-details-marker]:hidden"
          >
            Can't scan this card? Add it by name
            <span class="text-muted-foreground ml-auto text-xs group-open:hidden">Show</span>
            <span class="text-muted-foreground ml-auto hidden text-xs group-open:inline">Hide</span>
          </summary>
          <div class="border-t p-3">
            <QuickAddBox :game="game" />
          </div>
        </details>
      </section>
    </div>
  </div>
</template>

<style scoped>
@media (orientation: landscape) and (max-height: 31.25rem) {
  .scan-page-header {
    margin-bottom: 0.5rem;
  }

  .scan-page-header p {
    display: none;
  }
}

@media (orientation: landscape) and (max-width: 39.999rem) and (max-height: 31.25rem) {
  .scan-page-header {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  /* Reclaim the vertical space on small phones held sideways, where the header is hidden
     too. The setting is still honoured from its last (portrait) value. */
  .scan-auto-scroll-row {
    display: none;
  }
}
</style>
