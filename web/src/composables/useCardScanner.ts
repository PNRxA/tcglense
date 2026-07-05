import { onBeforeUnmount, ref, watch, type Ref } from 'vue'
// tesseract.js is a CommonJS `export =` namespace, so the type comes in via a default
// import (esModuleInterop) and the runtime via a lazy dynamic import().
import type Tesseract from 'tesseract.js'
import {
  NAME_REGION,
  SET_REGION,
  guideRect,
  regionInRect,
  type Rect,
} from '@/lib/scan/regions'

// Camera + on-device OCR engine for the card scanner. Owns the getUserMedia stream and a
// single, lazily-created Tesseract worker, and turns "the current video frame" into the
// two text strips the session needs (title + set/collector line). Everything CV-heavy is
// client-side: no image ever leaves the browser, matching the app's self-hosted posture.
//
// tesseract.js is a big WASM + trained-data payload, so it's dynamically imported the
// first time the camera starts (never at app load) and the worker is reused across frames.

export type CameraStatus = 'idle' | 'starting' | 'ready' | 'denied' | 'unavailable' | 'error'

/** The two OCR strips read from one captured frame. */
export interface ScanCapture {
  /** Raw OCR of the title bar. */
  nameText: string
  /** Raw OCR of the bottom-left collector/set line. */
  setText: string
}

// Only the characters the set/collector strip can contain — constraining the OCR here
// sharply cuts misreads of that tiny text. The name strip stays unconstrained (card names
// use a wide, accented character set).
const SET_WHITELIST = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/•· '

export function useCardScanner(video: Ref<HTMLVideoElement | null>) {
  const status = ref<CameraStatus>('idle')
  const errorMessage = ref<string | null>(null)
  const facingMode = ref<'environment' | 'user'>('environment')
  /** True while the OCR worker's WASM/trained-data is still downloading/initialising. */
  const ocrLoading = ref(false)

  let stream: MediaStream | null = null
  let worker: Tesseract.Worker | null = null
  let workerInit: Promise<Tesseract.Worker> | null = null
  // Set once the composable's component unmounts, so a late async resolution disposes its
  // resource instead of touching a torn-down component.
  let disposed = false
  // Bumped by stop()/switchCamera()/unmount so an in-flight start() whose getUserMedia
  // resolves late can tell it was superseded and stop the now-orphaned stream (getUserMedia
  // has no cancellation) rather than going live on a dead component.
  let generation = 0
  // One reusable scratch canvas for cropping frames — avoids churning nodes per capture.
  const canvas =
    typeof document !== 'undefined' ? document.createElement('canvas') : null

  function cameraSupported(): boolean {
    return typeof navigator !== 'undefined' && !!navigator.mediaDevices?.getUserMedia
  }

  async function attachStream() {
    const el = video.value
    if (el && stream && el.srcObject !== stream) {
      el.srcObject = stream
      // Autoplay can reject if the play() races the element mount; the stream still
      // attaches and metadata loads, so a rejection here is safe to swallow.
      try {
        await el.play()
      } catch {
        /* ignore */
      }
    }
  }

  // Reattach if the <video> mounts (or is replaced) after the stream is already live.
  watch(video, () => {
    void attachStream()
  })

  // The camera track ending on its own — permission revoked mid-session, device unplugged,
  // or the OS/another app reclaiming it — surfaces as an error and a teardown, rather than
  // leaving a live-looking viewport scanning a frozen/dead frame. (track.stop() does not
  // fire 'ended', so our own stop() won't re-enter here.)
  function onTrackEnded() {
    if (status.value !== 'ready' && status.value !== 'starting') return
    stop()
    status.value = 'error'
    errorMessage.value = 'The camera was disconnected. Start scanning again to retry.'
  }

  async function start(): Promise<void> {
    if (status.value === 'starting' || status.value === 'ready') return
    errorMessage.value = null
    if (!cameraSupported()) {
      status.value = 'unavailable'
      errorMessage.value =
        'Camera access needs a secure (HTTPS) connection and a supported browser.'
      return
    }
    const gen = ++generation
    status.value = 'starting'
    try {
      const media = await navigator.mediaDevices.getUserMedia({
        video: {
          facingMode: facingMode.value,
          width: { ideal: 1920 },
          height: { ideal: 1080 },
        },
        audio: false,
      })
      // Superseded while awaiting (stopped, camera switched, or unmounted): the stream we
      // just got is an orphan — stop its tracks and bail so the camera light goes off.
      if (gen !== generation || disposed) {
        for (const track of media.getTracks()) track.stop()
        return
      }
      stream = media
      for (const track of media.getVideoTracks()) track.addEventListener('ended', onTrackEnded)
      await attachStream()
      if (gen !== generation || disposed) {
        stop()
        return
      }
      status.value = 'ready'
      // Warm the OCR worker so the first real scan isn't stalled behind the download.
      void ensureWorker().catch(() => {})
    } catch (err) {
      if (gen !== generation) return // superseded — don't clobber a newer state
      stream = null
      const name = err instanceof DOMException ? err.name : ''
      if (name === 'NotAllowedError' || name === 'SecurityError') {
        status.value = 'denied'
        errorMessage.value =
          'Camera permission was denied. Allow camera access in your browser to scan.'
      } else if (
        name === 'NotFoundError' ||
        name === 'DevicesNotFoundError' ||
        name === 'OverconstrainedError'
      ) {
        status.value = 'unavailable'
        errorMessage.value = 'No camera was found on this device.'
      } else {
        status.value = 'error'
        errorMessage.value = 'Could not start the camera. Please try again.'
      }
    }
  }

  function stop(): void {
    // Invalidate any in-flight start() so its late getUserMedia resolution cleans itself up.
    generation++
    if (stream) {
      for (const track of stream.getTracks()) {
        track.removeEventListener('ended', onTrackEnded)
        track.stop()
      }
      stream = null
    }
    const el = video.value
    if (el) el.srcObject = null
    // Keep a terminal denied/unavailable/error state visible; otherwise return to idle.
    if (status.value === 'ready' || status.value === 'starting') status.value = 'idle'
  }

  async function switchCamera(): Promise<void> {
    facingMode.value = facingMode.value === 'environment' ? 'user' : 'environment'
    if (status.value === 'ready') {
      stop()
      await start()
    }
  }

  async function ensureWorker(): Promise<Tesseract.Worker> {
    if (worker) return worker
    if (!workerInit) {
      ocrLoading.value = true
      workerInit = import('tesseract.js')
        .then(({ createWorker }) => createWorker('eng'))
        .then((created) => {
          // Torn down while the worker warmed up — dispose it instead of leaking a thread.
          if (disposed) {
            void created.terminate()
            throw new Error('scanner disposed')
          }
          worker = created
          return created
        })
        .catch((err) => {
          // Don't cache a failed warm-up (e.g. the model download blipped): null the promise
          // so the next capture retries rather than reusing a permanently-rejected one.
          workerInit = null
          throw err
        })
        .finally(() => {
          ocrLoading.value = false
        })
    }
    return workerInit
  }

  // Crop one region of the current frame onto the scratch canvas — upscaled and
  // high-contrast grayscale — and OCR just that strip. A small, preprocessed region
  // reads far more reliably (and faster) than the whole frame.
  async function recognizeRect(
    rect: Rect,
    singleLine: boolean,
    whitelist: string,
  ): Promise<string> {
    const el = video.value
    if (!canvas || !el) return ''
    const scale = 2
    canvas.width = Math.max(1, Math.round(rect.width * scale))
    canvas.height = Math.max(1, Math.round(rect.height * scale))
    const ctx = canvas.getContext('2d')
    if (!ctx) return ''
    ctx.filter = 'grayscale(1) contrast(1.5) brightness(1.05)'
    ctx.drawImage(
      el,
      rect.left,
      rect.top,
      rect.width,
      rect.height,
      0,
      0,
      canvas.width,
      canvas.height,
    )
    const activeWorker = await ensureWorker()
    const { PSM } = await import('tesseract.js')
    await activeWorker.setParameters({
      // Single text line for the title; a block for the two-line set/collector strip.
      tessedit_pageseg_mode: singleLine ? PSM.SINGLE_LINE : PSM.SINGLE_BLOCK,
      // An empty whitelist disables the restriction (all characters allowed).
      tessedit_char_whitelist: whitelist,
    })
    const { data } = await activeWorker.recognize(canvas)
    return data.text ?? ''
  }

  /** OCR the title + set strips of the current frame, or null if the camera isn't ready,
   * the frame has no dimensions yet, or the OCR worker failed (transient — the next capture
   * retries, since a failed warm-up is no longer cached). */
  async function capture(): Promise<ScanCapture | null> {
    const el = video.value
    if (status.value !== 'ready' || !el) return null
    const vw = el.videoWidth
    const vh = el.videoHeight
    if (!vw || !vh) return null
    const guide = guideRect(vw, vh)
    try {
      const nameText = await recognizeRect(regionInRect(NAME_REGION, guide), true, '')
      const setText = await recognizeRect(regionInRect(SET_REGION, guide), false, SET_WHITELIST)
      return { nameText, setText }
    } catch {
      return null
    }
  }

  onBeforeUnmount(() => {
    disposed = true
    stop()
    const active = worker
    const pending = workerInit
    worker = null
    workerInit = null
    if (active) void active.terminate()
    // A worker still warming up: terminate it once it resolves (the disposed check above
    // also disposes it and rejects, which the catch swallows) so it never leaks.
    else if (pending) void pending.then((w) => w.terminate()).catch(() => {})
  })

  return {
    status,
    errorMessage,
    facingMode,
    ocrLoading,
    start,
    stop,
    switchCamera,
    capture,
  }
}
