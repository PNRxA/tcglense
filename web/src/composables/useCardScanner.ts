import { onBeforeUnmount, ref, watch, type Ref } from 'vue'
// tesseract.js is a CommonJS `export =` namespace, so the type comes in via a default
// import (esModuleInterop) and the runtime via a lazy dynamic import().
import type Tesseract from 'tesseract.js'
import { detectCardQuad, toGray, warpToRect, type Quad } from '@/lib/scan/detect'
import { detectCardQuadCv, loadOpenCv } from '@/lib/scan/opencvDetect'
import { phashFromRgba } from '@/lib/scan/phash'
import { SET_REGION, guideRect, regionInRect } from '@/lib/scan/regions'

// Camera + on-device vision engine for the card scanner. Owns the getUserMedia stream
// and turns "the current video frame" into what the session needs to identify the card:
// a perceptual-hash **fingerprint** of the deskewed card (which drives the visual match),
// plus an OCR read of the bottom-left set/collector line (which pins the exact printing).
// Everything CV-heavy is client-side — the photo never leaves the browser; only the
// 32-byte fingerprint is later sent to the match endpoint.
//
// The card is auto-detected and perspective-warped to a fixed upright crop (see
// `lib/scan/detect`); when detection fails (busy background, heavy rotation) it falls
// back to warping the on-screen guide box. tesseract.js is a big WASM + trained-data
// payload, so it's dynamically imported the first time the camera starts (never at app
// load) and its worker is reused; it now only reads the small set-line strip.

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
// let the server keep the closest. The base crop (rot 0, inset 0) is FIRST so
// `fingerprints[0]` is the canonical hash (used by the same-card stability gate).
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

/** What one captured frame yields for the session. */
export interface ScanCapture {
  /** 256-bit perceptual hashes of the deskewed card — the base crop plus small geometric
   * variants; the server matches whichever is closest to the reference. */
  fingerprints: Uint8Array[]
  /** Raw OCR of the bottom-left collector/set line — the printing hint. */
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
  /** True while OpenCV.js is still loading (detection uses the lightweight fallback until
   * then). */
  const cvLoading = ref(false)
  /** The card the live loop currently detects, as NORMALISED corners (0..1 of the frame),
   * or null when none is found — drives the on-screen outline and the capture crop. */
  const detectedQuad = ref<Quad | null>(null)

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

  // The guide box's four corners in frame pixels — the detection fallback: when no card
  // is detected we warp exactly the region the user aligned to.
  function guideQuad(frameW: number, frameH: number): Quad {
    const g = guideRect(frameW, frameH)
    return [
      { x: g.left, y: g.top },
      { x: g.left + g.width, y: g.top },
      { x: g.left + g.width, y: g.top + g.height },
      { x: g.left, y: g.top + g.height },
    ]
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
    const image = ctx.getImageData(0, 0, dw, dh)
    if (cv) {
      detectedQuad.value = detectCardQuadCv(cv, image)
    } else {
      const q = detectCardQuad(toGray(image.data, dw, dh), dw, dh)
      detectedQuad.value = q ? (q.map((p) => ({ x: p.x / dw, y: p.y / dh })) as Quad) : null
    }
  }

  function startDetectLoop(): void {
    if (detectTimer !== null) return
    detectTimer = window.setInterval(detectFrame, DETECT_INTERVAL_MS)
  }
  function stopDetectLoop(): void {
    if (detectTimer !== null) {
      clearInterval(detectTimer)
      detectTimer = null
    }
    detectedQuad.value = null
  }

  // Draw the current frame (downscaled), pick the card quad (the one the live loop is
  // tracking, else a one-off lightweight detect, else the guide box), and warp it to an
  // upright WARP_W×WARP_H crop. Null if the camera isn't ready / the frame has no size yet.
  function warpFrame(): Uint8ClampedArray | null {
    const el = video.value
    if (!frameCanvas || status.value !== 'ready' || !el) return null
    const vw = el.videoWidth
    const vh = el.videoHeight
    if (!vw || !vh) return null

    const scale = Math.min(1, FRAME_MAX / Math.max(vw, vh))
    const fw = Math.max(1, Math.round(vw * scale))
    const fh = Math.max(1, Math.round(vh * scale))
    frameCanvas.width = fw
    frameCanvas.height = fh
    const fctx = frameCanvas.getContext('2d', { willReadFrequently: true })
    if (!fctx) return null
    fctx.drawImage(el, 0, 0, fw, fh)
    const frame = fctx.getImageData(0, 0, fw, fh)

    // Prefer the card the live loop is tracking (what the outline shows), scaled to the
    // full-res frame; else a one-off lightweight detect; else the guide box.
    const norm = detectedQuad.value
    const quad: Quad = norm
      ? denormalizeQuad(norm, fw, fh)
      : (detectCardQuad(toGray(frame.data, fw, fh), fw, fh) ?? guideQuad(fw, fh))
    return warpToRect(frame.data, fw, fh, quad, WARP_W, WARP_H)
  }

  /** The fingerprint of the current frame's deskewed card, or null if the camera isn't
   * ready yet. Cheap (no OCR) — the live loop calls this every tick to gate on stability. */
  function captureFingerprint(): Uint8Array | null {
    const rgba = warpFrame()
    return rgba ? phashFromRgba(rgba, WARP_W, WARP_H) : null
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
  function variantFingerprints(): Uint8Array[] {
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

  // OCR the set/collector strip of the crop already on `cropCanvas`: crop out the
  // SET_REGION strip upscaled + high-contrast and recognise just that (a small,
  // preprocessed, now-deskewed region reads far more reliably than the raw frame did).
  async function recognizeSetLine(): Promise<string> {
    if (!cropCanvas || !ocrCanvas) return ''
    const rect = regionInRect(SET_REGION, { left: 0, top: 0, width: WARP_W, height: WARP_H })
    const scale = 2
    ocrCanvas.width = Math.max(1, Math.round(rect.width * scale))
    ocrCanvas.height = Math.max(1, Math.round(rect.height * scale))
    const octx = ocrCanvas.getContext('2d')
    if (!octx) return ''
    octx.filter = 'grayscale(1) contrast(1.5) brightness(1.05)'
    octx.drawImage(
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

  /** A full capture for committing a match: the pooled variant fingerprints of a short
   * burst of frames plus an OCR of the set line (which pins the exact printing). Null if
   * the camera isn't ready / no frame had dimensions; a transient OCR failure just yields
   * an empty `setText` (the printing then falls back to the newest, next capture retries). */
  async function capture(): Promise<ScanCapture | null> {
    const fingerprints: Uint8Array[] = []
    let lastRgba: Uint8ClampedArray | null = null
    for (let f = 0; f < BURST_FRAMES; f++) {
      const rgba = warpFrame()
      if (rgba && drawCropToCanvas(rgba)) {
        lastRgba = rgba
        for (const fp of variantFingerprints()) fingerprints.push(fp)
      }
      if (f < BURST_FRAMES - 1) await nextFrame()
    }
    if (!lastRgba) return null
    // Fallback if canvas variants were unavailable: at least the last crop's fingerprint.
    if (fingerprints.length === 0) fingerprints.push(phashFromRgba(lastRgba, WARP_W, WARP_H))
    // OCR the set line off the last frame's crop (still on cropCanvas).
    let setText = ''
    try {
      setText = await recognizeSetLine()
    } catch {
      setText = ''
    }
    return { fingerprints, setText }
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
    captureFingerprint,
  }
}
