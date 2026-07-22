import { onBeforeUnmount, ref, watch, type Ref } from 'vue'
// tesseract.js is a CommonJS `export =` namespace, so the type comes in via a default
// import (esModuleInterop) and the runtime via a lazy dynamic import().
import type Tesseract from 'tesseract.js'
import { createCardLock } from '@/lib/scan/cardLock'
import { detectCardQuad, quadArea, toGray, warpToRect, type Quad } from '@/lib/scan/detect'
import { cropImageData, priorSearchWindow } from '@/lib/scan/guidedDetect'
import { detectCardQuadCv, loadOpenCv } from '@/lib/scan/opencvDetect'
import { detectFoilStar } from '@/lib/scan/foilStar'
import { cornerMetrics } from '@/lib/scan/quadTracker'
import { hamming, phashFromRgba } from '@/lib/scan/phash'
import { guideTarget } from '@/lib/scan/quadSelect'
import { SET_REGION, regionInRect } from '@/lib/scan/regions'

// Camera + on-device vision engine for the card scanner. Owns the getUserMedia stream
// and turns "the current video frame" into what the session needs to identify the card:
// a perceptual-hash **fingerprint** of the deskewed card (which drives the visual match),
// plus an OCR read of the bottom-left set/collector line (which pins the exact printing).
// Everything CV-heavy is client-side — the photo never leaves the browser; only the
// 32-byte fingerprint is later sent to the match endpoint.
//
// The card is auto-detected and perspective-warped to a fixed upright crop (see
// `lib/scan/detect`); capture revalidates the live lock at higher resolution and refuses
// clipped or unstable geometry. tesseract.js is a big WASM + trained-data
// payload, so it's dynamically imported the first time the camera starts (never at app
// load) and its worker is reused; it now only reads the small set-line strip. Its
// worker/core/traineddata are self-hosted same-origin assets (see tesseractAssets() in
// vite.config.ts), never a third-party CDN.

export type CameraStatus = 'idle' | 'starting' | 'ready' | 'denied' | 'unavailable' | 'error'

/** Size of the upright, deskewed crop the card is warped to (61:85, matching the guide
 * and the reference images the index is built from). */
const WARP_W = 610
const WARP_H = 850

/** Cap on the frame's long edge used for detection — keeps the per-frame CV cheap on a
 * mid phone without hurting corner accuracy. */
const FRAME_MAX = 1000

// Geometric variants of each deskewed crop the match query carries. A hand-held scan has
// residual rotation and small framing error even after detection+warp, and pHash is very
// sensitive to both, so we hash a small grid of rotations × inset (zoom) corrections and
// let the server keep the closest. The base crop (rot 0, inset 0) stays first so callers
// can treat `fingerprints[0]` as the canonical hash.
const VARIANT_ROTATIONS = [0, -5, 5]
const VARIANT_INSETS = [0, 0.05, 0.1]

// Crop quality varies a lot frame-to-frame on a hand-held card (perspective from the hold
// angle, focus, glare) — a card that scores 85 one frame scores 40 the next. So a single
// capture pools a short BURST of frames (each × its variants) and the server keeps the
// best match, turning "some frames match" into "the capture matches if any recent frame
// was good". Only runs on a commit, so the cost is fine.
const BURST_FRAMES = 4

/** Long edge (px) the live frame is downscaled to for continuous detection — small
 * enough that OpenCV runs at a smooth few-fps, large enough for accurate corners. */
const DETECT_MAX = 640

/** How often the live detection loop runs (ms) — ~8 fps for a responsive outline. */
const DETECT_INTERVAL_MS = 120

/** While nothing is detected (and no prior narrows the search), the expensive fallback
 * passes (extra Canny bands + the Otsu segmentation retry) run only every Nth
 * consecutive miss: a cardless viewfinder is the scanner's dominant state, and the
 * fallbacks multiply a cardless tick's cost. The outline tracker's hold bridges a
 * lock that lands a tick or two later, so the cadence is invisible on screen. Ticks
 * that search a prior's small region of interest always run the full ladder. */
const FALLBACK_MISS_CADENCE = 3

/** With a prior, one consecutive ROI miss is tolerated before a full-frame retry —
 * the card usually just moved within (or slightly out of) the padded ROI. The
 * full-frame retry stays associated to the prior, so an unrelated object across the
 * frame can never take the lock over. */
const PRIOR_MISSES_BEFORE_FULL_FRAME = 1

/** Capture-time geometry must agree with the green lock before its crop is accepted. */
const CAPTURE_MEAN_DISTANCE = 0.05
const CAPTURE_MAX_CORNER_DISTANCE = 0.08
const CAPTURE_MIN_AREA_RATIO = 0.9
const CAPTURE_MAX_AREA_RATIO = 1.1
/** Adjacent burst frames should be much closer than an arbitrary catalog match. This is
 * deliberately far stricter than the server's 96-bit recognition radius: pooling two
 * identities is worse than dropping a useful-but-noisy later frame. */
const CAPTURE_SAME_CARD_MAX_DISTANCE = 32

/** What one captured frame yields for the session. */
export interface ScanCapture {
  /** 256-bit perceptual hashes of the deskewed card — the base crop plus small geometric
   * variants; the server matches whichever is closest to the reference. */
  fingerprints: Uint8Array[]
  /** Raw OCR of the bottom-left collector/set line — the printing hint. */
  setText: string
  /** Whether a printed foil star was found on the card's info line (see `lib/scan/foilStar`).
   * False when OpenCV isn't loaded yet, the crop is too noisy, or the card prints no star. */
  foil: boolean
}

// Only the characters the set/collector strip can contain — constraining the OCR here
// sharply cuts misreads of that tiny text. The name strip stays unconstrained (card names
// use a wide, accented character set). (The foil star `★` is deliberately absent: tesseract's
// eng model has no such glyph and can never emit it — the star is found visually instead, see
// `lib/scan/foilStar` + the `foil` field below.)
const SET_WHITELIST = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/•· '

export function useCardScanner(video: Ref<HTMLVideoElement | null>) {
  const status = ref<CameraStatus>('idle')
  const errorMessage = ref<string | null>(null)
  const facingMode = ref<'environment' | 'user'>('environment')
  /** True while the OCR worker's WASM/trained-data is still downloading/initialising. */
  const ocrLoading = ref(false)
  /** True while OpenCV.js is still loading (detection uses the lightweight fallback until
   * then). */
  const cvLoading = ref(false)
  /** The card the live loop currently detects, as NORMALISED corners (0..1 of the frame),
   * or null when none is found — drives the on-screen outline and the capture crop. */
  const detectedQuad = ref<Quad | null>(null)
  // Owns lock acquisition and stability: a first detection stays tentative (no green)
  // until a consistent second confirms it, then the wrapped quadTracker smooths corner
  // noise, snaps on a real move, and bridges brief dropouts. Also the source of the
  // spatial prior the next tick searches around (see lib/scan/cardLock).
  const lock = createCardLock()
  // Consecutive live-loop ticks with no raw detection — the counter behind the
  // fallback-pass cadence (a detection resets it, so a card that only a fallback pass
  // sees keeps being re-detected every tick while it stays in frame).
  let rawMisses = 0
  // Consecutive prior-ROI misses — after tolerating one, the tick retries full-frame
  // (still associated to the prior) in case the card moved out of the ROI.
  let priorMisses = 0

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
  // Reusable scratch canvases — avoid churning DOM nodes per captured frame. `frameCanvas`
  // holds the (downscaled) video frame we detect in; `cropCanvas` holds the warped card;
  // `ocrCanvas` holds the upscaled set-line strip handed to Tesseract.
  const makeCanvas = () =>
    typeof document !== 'undefined' ? document.createElement('canvas') : null
  const frameCanvas = makeCanvas()
  const cropCanvas = makeCanvas()
  const ocrCanvas = makeCanvas()
  // Holds each rotated/inset variant while its fingerprint is computed.
  const variantCanvas = makeCanvas()
  // Small canvas the live detection loop reads (downscaled to DETECT_MAX).
  const detectCanvas = makeCanvas()

  // The loaded OpenCV runtime (null until ready), a guard so it's only loaded once, and
  // the live detection loop's timer.
  let cv: Awaited<ReturnType<typeof loadOpenCv>> | null = null
  let cvRequested = false
  let detectTimer: number | null = null

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
      // Load OpenCV + start the live card-detection outline (uses the lightweight
      // detector until OpenCV is ready).
      ensureCv()
      startDetectLoop()
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
    stopDetectLoop()
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
    // Restart while the camera is live OR still coming up: a tap during the (slow, on mobile)
    // 'starting' window must still take effect, or facingMode desyncs from the live camera.
    // stop() bumps the generation (so the in-flight start()'s late getUserMedia self-cleans)
    // and flips 'starting' → 'idle', so this start() passes its guard and re-reads facingMode.
    if (status.value === 'ready' || status.value === 'starting') {
      stop()
      await start()
    }
  }

  async function ensureWorker(): Promise<Tesseract.Worker> {
    if (worker) return worker
    if (!workerInit) {
      ocrLoading.value = true
      workerInit = import('tesseract.js')
        .then(({ createWorker, OEM }) =>
          // Explicit same-origin asset paths — the files the tesseractAssets() plugin in
          // vite.config.ts publishes under /tesseract/ — instead of tesseract.js's
          // cdn.jsdelivr.net defaults, so no third-party-served code ever runs with the
          // app origin's privileges (issue #451). OEM.LSTM_ONLY is the library default,
          // but it's load-bearing here: only the LSTM cores and traineddata are
          // published. workerBlobURL: false makes the worker a plain same-origin script
          // instead of a Blob wrapper, so a `worker-src 'self'` CSP (#450) can hold.
          createWorker('eng', OEM.LSTM_ONLY, {
            workerPath: `${import.meta.env.BASE_URL}tesseract/worker.min.js`,
            corePath: `${import.meta.env.BASE_URL}tesseract/core`,
            langPath: `${import.meta.env.BASE_URL}tesseract/lang`,
            workerBlobURL: false,
          }),
        )
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

  /** Scale a normalised quad (0..1) up to the pixel coords of a `w`×`h` frame. */
  function denormalizeQuad(quad: Quad, w: number, h: number): Quad {
    return quad.map((p) => ({ x: p.x * w, y: p.y * h })) as Quad
  }

  // Warm the OpenCV runtime once (detection uses the lightweight detector until it's
  // ready). A load failure is swallowed — the fallback keeps the scanner working.
  function ensureCv(): void {
    if (cv || cvRequested) return
    cvRequested = true
    cvLoading.value = true
    loadOpenCv()
      .then((loaded) => {
        if (!disposed) cv = loaded
      })
      .catch(() => {
        cvRequested = false // allow a retry on the next start
      })
      .finally(() => {
        cvLoading.value = false
      })
  }

  // One live-detection step: downscale the current frame and find the card's corners,
  // updating `detectedQuad` (normalised) so the outline tracks the card. OpenCV when
  // loaded, else the lightweight detector. Runs on a timer while the camera is live.
  //
  // With a prior (a tentative or locked card), only a padded region of interest around
  // it is read and searched, in tracking mode: clutter outside the ROI vanishes from
  // the search — the busy-background lock keeps holding — and the smaller crop is
  // cheap enough to always run the full detection ladder. Without a prior, the full
  // frame is searched in acquisition mode (guide-weighted, ambiguity-rejecting), with
  // the expensive fallback passes on a cadence of misses.
  function detectFrame(): void {
    const el = video.value
    if (!detectCanvas || status.value !== 'ready' || !el) return
    const vw = el.videoWidth
    const vh = el.videoHeight
    if (!vw || !vh) return
    const scale = Math.min(1, DETECT_MAX / Math.max(vw, vh))
    const dw = Math.max(1, Math.round(vw * scale))
    const dh = Math.max(1, Math.round(vh * scale))
    detectCanvas.width = dw
    detectCanvas.height = dh
    const ctx = detectCanvas.getContext('2d', { willReadFrequently: true })
    if (!ctx) return
    ctx.drawImage(el, 0, 0, dw, dh)
    let raw: Quad | null
    if (cv) {
      const prior = lock.prior()
      if (prior) {
        const roi = priorSearchWindow(prior, dw, dh)
        raw = detectCardQuadCv(cv, ctx.getImageData(roi.x, roi.y, roi.width, roi.height), {
          window: roi,
          select: { mode: 'tracking', prior },
          fallbackPasses: true,
        })
        if (!raw && priorMisses >= PRIOR_MISSES_BEFORE_FULL_FRAME) {
          raw = detectCardQuadCv(cv, ctx.getImageData(0, 0, dw, dh), {
            select: { mode: 'tracking', prior },
            fallbackPasses: false,
          })
        }
        priorMisses = raw ? 0 : priorMisses + 1
      } else {
        // Just dropped out of prior mode (lock lost/invalidated): restart the miss
        // cadence so the first cold acquisition tick gets the full ladder instead of
        // inheriting a stale count from ROI ticks that never used it.
        if (priorMisses > 0) rawMisses = 0
        priorMisses = 0
        raw = detectCardQuadCv(cv, ctx.getImageData(0, 0, dw, dh), {
          select: { mode: 'acquisition', guide: guideTarget(dw, dh) },
          fallbackPasses: rawMisses % FALLBACK_MISS_CADENCE === 0,
        })
      }
    } else {
      const image = ctx.getImageData(0, 0, dw, dh)
      const q = detectCardQuad(toGray(image.data, dw, dh), dw, dh)
      raw = q ? (q.map((p) => ({ x: p.x / dw, y: p.y / dh })) as Quad) : null
    }
    rawMisses = raw ? 0 : rawMisses + 1
    detectedQuad.value = lock.update(raw, performance.now())
  }

  // A chained timeout, not an interval: a detection tick that overruns its budget on a
  // slow device must delay the next tick rather than queue back-to-back callbacks. The
  // finally keeps the chain alive through a transiently throwing tick (a mid-resize
  // getImageData, an OpenCV hiccup) — like setInterval, one bad frame must not
  // silently freeze the outline for the rest of the session.
  function scheduleDetect(): void {
    detectTimer = window.setTimeout(() => {
      try {
        detectFrame()
      } finally {
        if (detectTimer !== null) scheduleDetect()
      }
    }, DETECT_INTERVAL_MS)
  }
  function startDetectLoop(): void {
    if (detectTimer !== null) return
    scheduleDetect()
  }
  function stopDetectLoop(): void {
    if (detectTimer !== null) {
      clearTimeout(detectTimer)
      detectTimer = null
    }
    lock.reset()
    rawMisses = 0
    priorMisses = 0
    detectedQuad.value = null
  }

  /** Read one current video frame at the bounded capture resolution. */
  function readFrame(): ImageData | null {
    const el = video.value
    if (!frameCanvas || status.value !== 'ready' || !el) return null
    const vw = el.videoWidth
    const vh = el.videoHeight
    if (!vw || !vh) return null

    const scale = Math.min(1, FRAME_MAX / Math.max(vw, vh))
    const width = Math.max(1, Math.round(vw * scale))
    const height = Math.max(1, Math.round(vh * scale))
    frameCanvas.width = width
    frameCanvas.height = height
    const context = frameCanvas.getContext('2d', { willReadFrequently: true })
    if (!context) return null
    context.drawImage(el, 0, 0, width, height)
    return context.getImageData(0, 0, width, height)
  }

  /** Detect a fresh normalised quad on the capture-resolution frame. With a prior (the
   * live lock, or the previous accepted burst quad), the search runs prior-associated
   * capture selection over the prior's region of interest first, retrying full-frame —
   * so whatever pass produced the live lock on a busy background is reproducible here,
   * and an unassociated rectangle elsewhere in the frame can never be "the card". */
  function detectCaptureQuad(frame: ImageData, prior: Quad | null): Quad | null {
    if (cv) {
      if (prior) {
        const roi = priorSearchWindow(prior, frame.width, frame.height)
        return (
          detectCardQuadCv(cv, cropImageData(frame, roi), {
            window: roi,
            select: { mode: 'capture', prior },
            fallbackPasses: true,
          }) ??
          detectCardQuadCv(cv, frame, { select: { mode: 'capture', prior }, fallbackPasses: true })
        )
      }
      return detectCardQuadCv(cv, frame, { fallbackPasses: true })
    }
    const quad = detectCardQuad(
      toGray(frame.data, frame.width, frame.height),
      frame.width,
      frame.height,
    )
    return quad
      ? (quad.map((point) => ({
          x: point.x / frame.width,
          y: point.y / frame.height,
        })) as Quad)
      : null
  }

  function captureQuadAgrees(tracked: Quad, candidate: Quad): boolean {
    const trackedArea = quadArea(tracked)
    if (trackedArea <= 0) return false
    const areaRatio = quadArea(candidate) / trackedArea
    const metrics = cornerMetrics(tracked, candidate)
    return (
      metrics.mean <= CAPTURE_MEAN_DISTANCE &&
      metrics.max <= CAPTURE_MAX_CORNER_DISTANCE &&
      areaRatio >= CAPTURE_MIN_AREA_RATIO &&
      areaRatio <= CAPTURE_MAX_AREA_RATIO
    )
  }

  function invalidateLock(): void {
    lock.reset()
    // Fresh acquisition deserves a fresh cadence — the very next tick may need the
    // fallback bands to re-find the card the capture just distrusted.
    rawMisses = 0
    priorMisses = 0
    detectedQuad.value = null
  }

  /** Warp one captured frame with an already validated normalised quad. */
  function warpFrame(frame: ImageData, quad: Quad): Uint8ClampedArray {
    return warpToRect(
      frame.data,
      frame.width,
      frame.height,
      denormalizeQuad(quad, frame.width, frame.height),
      WARP_W,
      WARP_H,
    )
  }

  // Draw the deskewed crop onto `cropCanvas` — the shared source for the variant
  // fingerprints and the set-line OCR. Copies into a fresh ArrayBuffer-backed array,
  // since `ImageData` needs `Uint8ClampedArray<ArrayBuffer>`, not the warp buffer's
  // `ArrayBufferLike`.
  function drawCropToCanvas(rgba: Uint8ClampedArray): boolean {
    if (!cropCanvas) return false
    cropCanvas.width = WARP_W
    cropCanvas.height = WARP_H
    const ctx = cropCanvas.getContext('2d')
    if (!ctx) return false
    ctx.putImageData(new ImageData(new Uint8ClampedArray(rgba), WARP_W, WARP_H), 0, 0)
    return true
  }

  // Fingerprint the crop on `cropCanvas` plus a grid of rotation × inset (zoom) variants,
  // so a residually-rotated or slightly-loose scan still matches its tight, upright
  // reference (the server keeps the closest). The base crop (rot 0, inset 0) is included.
  function variantFingerprints(baseFingerprint?: Uint8Array): Uint8Array[] {
    const out: Uint8Array[] = []
    if (!cropCanvas || !variantCanvas) return out
    variantCanvas.width = WARP_W
    variantCanvas.height = WARP_H
    const ctx = variantCanvas.getContext('2d', { willReadFrequently: true })
    if (!ctx) return out
    for (const inset of VARIANT_INSETS) {
      const sx = inset * WARP_W
      const sy = inset * WARP_H
      const sw = WARP_W - 2 * sx
      const sh = WARP_H - 2 * sy
      for (const rot of VARIANT_ROTATIONS) {
        if (inset === 0 && rot === 0 && baseFingerprint) {
          out.push(baseFingerprint)
          continue
        }
        ctx.save()
        ctx.clearRect(0, 0, WARP_W, WARP_H)
        if (rot !== 0) {
          ctx.translate(WARP_W / 2, WARP_H / 2)
          ctx.rotate((rot * Math.PI) / 180)
          ctx.translate(-WARP_W / 2, -WARP_H / 2)
        }
        // Draw the inset source region to the full canvas (zoom), under the rotation.
        ctx.drawImage(cropCanvas, sx, sy, sw, sh, 0, 0, WARP_W, WARP_H)
        ctx.restore()
        out.push(phashFromRgba(ctx.getImageData(0, 0, WARP_W, WARP_H).data, WARP_W, WARP_H))
      }
    }
    return out
  }

  // Copy the validated first crop's set/collector strip before later burst frames reuse
  // cropCanvas. The small upscaled, high-contrast region reads much better than raw video.
  function prepareSetLine(): boolean {
    if (!cropCanvas || !ocrCanvas) return false
    const rect = regionInRect(SET_REGION, { left: 0, top: 0, width: WARP_W, height: WARP_H })
    const scale = 2
    ocrCanvas.width = Math.max(1, Math.round(rect.width * scale))
    ocrCanvas.height = Math.max(1, Math.round(rect.height * scale))
    const context = ocrCanvas.getContext('2d')
    if (!context) return false
    context.filter = 'grayscale(1) contrast(1.5) brightness(1.05)'
    context.drawImage(
      cropCanvas,
      rect.left,
      rect.top,
      rect.width,
      rect.height,
      0,
      0,
      ocrCanvas.width,
      ocrCanvas.height,
    )
    return true
  }

  // Look for the printed foil star on the validated first crop (still on cropCanvas, before
  // the burst loop reuses it). OpenCV-only and best-effort: a not-yet-loaded runtime, a
  // missing canvas, or any detector error just means "no star" (a regular copy), never a throw
  // on the capture path.
  function detectStarFromCrop(): boolean {
    if (!cv || !cropCanvas) return false
    const context = cropCanvas.getContext('2d', { willReadFrequently: true })
    if (!context) return false
    try {
      // The loaded runtime is fully-featured; opencvDetect's `Cv` types only the calls it
      // makes, so widen it to the (larger) surface detectFoilStar needs.
      const runtime = cv as unknown as Parameters<typeof detectFoilStar>[0]
      return detectFoilStar(runtime, context.getImageData(0, 0, WARP_W, WARP_H))
    } catch {
      return false
    }
  }

  async function recognizeSetLine(): Promise<string> {
    if (!ocrCanvas) return ''
    const activeWorker = await ensureWorker()
    const { PSM } = await import('tesseract.js')
    await activeWorker.setParameters({
      // A block for the two-line set/collector strip (number over set code / language).
      tessedit_pageseg_mode: PSM.SINGLE_BLOCK,
      tessedit_char_whitelist: SET_WHITELIST,
    })
    const { data } = await activeWorker.recognize(ocrCanvas)
    return data.text ?? ''
  }

  /** Wait for the next animation frame so the camera can advance between burst grabs. */
  function nextFrame(): Promise<void> {
    return new Promise((resolve) => {
      if (typeof requestAnimationFrame === 'function') requestAnimationFrame(() => resolve())
      else setTimeout(resolve, 30)
    })
  }

  /** A full capture for committing a match: validate the green lock at capture resolution,
   * include only burst frames that remain the same card, then OCR the validated first crop.
   * Null means the first frame became clipped or no longer agrees with the displayed lock. */
  async function capture(): Promise<ScanCapture | null> {
    const tracked = detectedQuad.value
    const firstFrame = readFrame()
    if (!tracked || !firstFrame) return null
    const firstQuad = detectCaptureQuad(firstFrame, tracked)
    if (!firstQuad || !captureQuadAgrees(tracked, firstQuad)) {
      invalidateLock()
      return null
    }

    const validatedRgba = warpFrame(firstFrame, firstQuad)
    const referenceFingerprint = phashFromRgba(validatedRgba, WARP_W, WARP_H)
    if (!drawCropToCanvas(validatedRgba)) return null
    const setLineReady = prepareSetLine()
    // Read the foil star off this first crop before the burst loop overwrites cropCanvas.
    const foil = detectStarFromCrop()
    const fingerprints = variantFingerprints(referenceFingerprint)
    // Fallback if canvas variants were unavailable: keep the geometry-validated base crop.
    if (fingerprints.length === 0) fingerprints.push(referenceFingerprint)

    let burstPrior = firstQuad
    for (let f = 1; f < BURST_FRAMES; f++) {
      await nextFrame()
      const frame = readFrame()
      if (!frame) break
      // ROI around the previous accepted quad; agreement stays anchored to the FIRST
      // quad so small per-frame drift cannot accumulate across the burst.
      const quad = detectCaptureQuad(frame, burstPrior)
      if (!quad || !captureQuadAgrees(firstQuad, quad)) break
      burstPrior = quad
      const rgba = warpFrame(frame, quad)
      const fingerprint = phashFromRgba(rgba, WARP_W, WARP_H)
      if (hamming(referenceFingerprint, fingerprint) > CAPTURE_SAME_CARD_MAX_DISTANCE) break
      if (!drawCropToCanvas(rgba)) break
      for (const variant of variantFingerprints(fingerprint)) fingerprints.push(variant)
    }

    let setText = ''
    if (setLineReady) {
      try {
        setText = await recognizeSetLine()
      } catch {
        setText = ''
      }
    }
    return { fingerprints, setText, foil }
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
    cvLoading,
    detectedQuad,
    start,
    stop,
    switchCamera,
    capture,
  }
}
