import { defineComponent, ref, type Ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Quad } from '@/lib/scan/detect'

const mocks = vi.hoisted(() => {
  const worker = {
    setParameters: vi.fn<() => Promise<object>>(async () => ({})),
    recognize: vi.fn<() => Promise<{ data: { text: string } }>>(async () => ({
      data: { text: '' },
    })),
    terminate: vi.fn<() => Promise<object>>(async () => ({})),
  }
  return {
    worker,
    createWorker: vi.fn<() => Promise<typeof worker>>(async () => worker),
    // Pending-forever by default (the OCR-focused specs never need the CV runtime);
    // the detection-loop specs override it to resolve.
    loadOpenCv: vi.fn<() => Promise<unknown>>(() => new Promise<never>(() => {})),
    detectCardQuadCv: vi.fn<
      (cv: unknown, image: unknown, opts: { segmentationFallback: boolean }) => Quad | null
    >(() => null),
    hamming: vi.fn<(a: Uint8Array, b: Uint8Array) => number>(() => 0),
  }
})

// The OCR engine and OpenCV runtime are heavyweight wasm payloads — mock both; this
// spec only locks the wiring around them.
vi.mock('tesseract.js', () => ({
  createWorker: mocks.createWorker,
  OEM: { LSTM_ONLY: 1 },
  PSM: { SINGLE_BLOCK: '6' },
}))
vi.mock('@/lib/scan/opencvDetect', () => ({
  loadOpenCv: mocks.loadOpenCv,
  detectCardQuadCv: mocks.detectCardQuadCv,
}))
vi.mock('@/lib/scan/phash', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/scan/phash')>()
  return { ...actual, hamming: mocks.hamming }
})

import { useCardScanner } from '../useCardScanner'

function mountScanner(video: Ref<HTMLVideoElement | null> = ref(null)) {
  let scanner!: ReturnType<typeof useCardScanner>
  const wrapper = mount(
    defineComponent({
      setup() {
        scanner = useCardScanner(video)
        return () => null
      },
    }),
  )
  return { scanner, wrapper }
}

function mockCamera() {
  const fakeStream = { getTracks: () => [], getVideoTracks: () => [] }
  Object.defineProperty(navigator, 'mediaDevices', {
    configurable: true,
    value: {
      getUserMedia: vi.fn<() => Promise<typeof fakeStream>>(async () => fakeStream),
    },
  })
}

afterEach(() => {
  vi.clearAllMocks()
  vi.useRealTimers()
  vi.unstubAllGlobals()
  Reflect.deleteProperty(navigator, 'mediaDevices')
})

describe('useCardScanner OCR worker', () => {
  it('creates the tesseract worker from self-hosted same-origin assets, never a CDN', async () => {
    mockCamera()
    const { scanner, wrapper } = mountScanner()
    await scanner.start()
    await flushPromises()

    // The load-bearing contract of issue #451: explicit same-origin paths (the files
    // vite.config.ts's tesseractAssets() publishes) — without them tesseract.js falls
    // back to downloading cdn.jsdelivr.net code that runs with the app origin's
    // privileges. LSTM_ONLY is pinned because only the LSTM cores are published.
    expect(mocks.createWorker).toHaveBeenCalledTimes(1)
    expect(mocks.createWorker).toHaveBeenCalledWith(
      'eng',
      1,
      expect.objectContaining({
        workerPath: '/tesseract/worker.min.js',
        corePath: '/tesseract/core',
        langPath: '/tesseract/lang',
        workerBlobURL: false,
      }),
    )

    // The warmed worker is still disposed with the component.
    wrapper.unmount()
    await flushPromises()
    expect(mocks.worker.terminate).toHaveBeenCalled()
  })
})

describe('useCardScanner live detection loop', () => {
  /** An axis-aligned normalised quad at (x, y). */
  function quadAt(x: number, y: number, w = 0.4, h = 0.56): Quad {
    return [
      { x, y },
      { x: x + w, y },
      { x: x + w, y: y + h },
      { x, y: y + h },
    ]
  }

  /** A fake <video> with frame dimensions, plus canvas 2D stubs so detectFrame can
   * draw and read a frame in jsdom (which has no real canvas backend). */
  function mockVideoAndCanvas() {
    const el = {
      srcObject: null as unknown,
      play: vi.fn<() => Promise<void>>(async () => {}),
      videoWidth: 640,
      videoHeight: 480,
    }
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({
      drawImage: () => {},
      putImageData: () => {},
      clearRect: () => {},
      save: () => {},
      restore: () => {},
      translate: () => {},
      rotate: () => {},
      getImageData: (x: number, y: number, w: number, h: number) => ({
        data: new Uint8ClampedArray(w * h * 4),
        width: w,
        height: h,
      }),
    } as unknown as CanvasRenderingContext2D)
    return ref(el as unknown as HTMLVideoElement)
  }

  function mockBurstFrameEnvironment() {
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      callback(0)
      return 1
    })
    vi.stubGlobal(
      'ImageData',
      class {
        constructor(
          public data: Uint8ClampedArray,
          public width: number,
          public height: number,
        ) {}
      },
    )
  }

  async function startLive() {
    mockCamera()
    mocks.loadOpenCv.mockResolvedValue({}) // a truthy stand-in runtime
    const video = mockVideoAndCanvas()
    const { scanner, wrapper } = mountScanner(video)
    await scanner.start()
    await flushPromises() // lets the mocked OpenCV runtime finish "loading"
    return { scanner, wrapper }
  }

  it('smooths nearby detections, holds through 3 missed ticks, then clears', async () => {
    vi.useFakeTimers()
    const { scanner, wrapper } = await startLive()

    const first = quadAt(0.2, 0.15)
    const jittered = quadAt(0.22, 0.15)
    mocks.detectCardQuadCv
      .mockReturnValueOnce(first)
      .mockReturnValueOnce(jittered)
      .mockReturnValue(null)

    vi.advanceTimersByTime(120)
    expect(scanner.detectedQuad.value).toEqual(first)

    // A near detection is blended (default 0.5), not adopted raw — the outline
    // moves to the midpoint instead of twitching.
    vi.advanceTimersByTime(120)
    expect(scanner.detectedQuad.value![0]!.x).toBeCloseTo(0.21, 10)

    // Three consecutive misses are bridged by the hold…
    for (let miss = 0; miss < 3; miss++) {
      vi.advanceTimersByTime(120)
      expect(scanner.detectedQuad.value).not.toBeNull()
    }
    // …the fourth clears the outline.
    vi.advanceTimersByTime(120)
    expect(scanner.detectedQuad.value).toBeNull()

    wrapper.unmount()
  })

  it('runs the Otsu retry on a 1-in-3 miss cadence, and always after a detection', async () => {
    vi.useFakeTimers()
    const { wrapper } = await startLive()

    mocks.detectCardQuadCv
      .mockReturnValueOnce(quadAt(0.2, 0.15))
      .mockReturnValueOnce(quadAt(0.2, 0.15))
      .mockReturnValue(null)

    for (let tick = 0; tick < 7; tick++) vi.advanceTimersByTime(120)

    // Ticks 1-2 detect (misses stay 0 → retry allowed), tick 3 misses (still the
    // 0th miss → allowed), then the cadence gates 2 of every 3 cardless ticks.
    expect(mocks.detectCardQuadCv.mock.calls.map((call) => call[2].segmentationFallback)).toEqual([
      true,
      true,
      true,
      false,
      false,
      true,
      false,
    ])

    wrapper.unmount()
  })

  it('refuses capture when fresh full-resolution geometry no longer matches the lock', async () => {
    vi.useFakeTimers()
    const { scanner, wrapper } = await startLive()
    const locked = quadAt(0.2, 0.15)
    const oneCornerAtEdge: Quad = locked.map((point) => ({ ...point })) as Quad
    oneCornerAtEdge[0] = { x: 0, y: 0 }

    mocks.detectCardQuadCv.mockReturnValueOnce(locked).mockReturnValueOnce(oneCornerAtEdge)
    vi.advanceTimersByTime(120)
    expect(scanner.detectedQuad.value).toEqual(locked)

    expect(await scanner.capture()).toBeNull()
    expect(scanner.detectedQuad.value).toBeNull()
    expect(mocks.detectCardQuadCv).toHaveBeenLastCalledWith(
      expect.anything(),
      expect.objectContaining({ width: 640, height: 480 }),
      { segmentationFallback: true },
    )
    // Strong capture-time evidence invalidates the stale green hold immediately, but the
    // next good live frame can reacquire without waiting out the old miss window.
    mocks.detectCardQuadCv.mockReturnValue(locked)
    vi.advanceTimersByTime(120)
    expect(scanner.detectedQuad.value).toEqual(locked)

    wrapper.unmount()
  })

  it('rejects a coherent inner-frame crop that would omit the card border', async () => {
    vi.useFakeTimers()
    const { scanner, wrapper } = await startLive()
    const outer = quadAt(0.2, 0.1, 0.6, 0.8)
    const inner = quadAt(0.224, 0.132, 0.552, 0.736)

    mocks.detectCardQuadCv.mockReturnValueOnce(outer).mockReturnValueOnce(inner)
    vi.advanceTimersByTime(120)

    expect(await scanner.capture()).toBeNull()
    expect(scanner.detectedQuad.value).toBeNull()
    wrapper.unmount()
  })

  it('validates every accepted frame while keeping one quad per burst frame', async () => {
    vi.useFakeTimers()
    mockBurstFrameEnvironment()
    const { scanner, wrapper } = await startLive()
    const locked = quadAt(0.2, 0.15)
    mocks.detectCardQuadCv.mockReturnValue(locked)
    vi.advanceTimersByTime(120)
    const callsBeforeCapture = mocks.detectCardQuadCv.mock.calls.length

    const captured = await scanner.capture()

    expect(captured).not.toBeNull()
    expect(captured!.fingerprints).toHaveLength(36)
    // Every pooled frame has fresh high-resolution geometry; the live tracked quad is
    // only the initial reference and cannot mutate the in-progress burst.
    expect(mocks.detectCardQuadCv).toHaveBeenCalledTimes(callsBeforeCapture + 4)
    expect(mocks.detectCardQuadCv).toHaveBeenLastCalledWith(
      expect.anything(),
      expect.objectContaining({ width: 640, height: 480 }),
      { segmentationFallback: true },
    )

    wrapper.unmount()
  })

  it('stops pooling when a later burst frame moves away from the validated card', async () => {
    vi.useFakeTimers()
    mockBurstFrameEnvironment()
    const { scanner, wrapper } = await startLive()
    const locked = quadAt(0.2, 0.15)
    const moved = quadAt(0.4, 0.15)
    mocks.detectCardQuadCv
      .mockReturnValueOnce(locked) // live lock
      .mockReturnValueOnce(locked) // first capture frame
      .mockReturnValueOnce(moved) // next frame: stop the burst
    vi.advanceTimersByTime(120)
    const callsBeforeCapture = mocks.detectCardQuadCv.mock.calls.length

    const captured = await scanner.capture()

    expect(captured).not.toBeNull()
    expect(captured!.fingerprints).toHaveLength(9)
    expect(mocks.detectCardQuadCv).toHaveBeenCalledTimes(callsBeforeCapture + 2)
    wrapper.unmount()
  })

  it('does not pool a later frame from another card at the same geometry', async () => {
    vi.useFakeTimers()
    mockBurstFrameEnvironment()
    const { scanner, wrapper } = await startLive()
    const locked = quadAt(0.2, 0.15)
    mocks.detectCardQuadCv.mockReturnValue(locked)
    // Geometry alone cannot distinguish a rapid replacement at the same position. A
    // 64-bit pHash change is within the server's broad match radius but not same-frame
    // continuity, so only the first card's variants may survive.
    mocks.hamming.mockReturnValueOnce(64)
    vi.advanceTimersByTime(120)

    const captured = await scanner.capture()

    expect(captured).not.toBeNull()
    expect(captured!.fingerprints).toHaveLength(9)
    expect(mocks.hamming).toHaveBeenCalledTimes(1)
    wrapper.unmount()
  })

  it('stop() forgets the track — no stale outline can survive a restart', async () => {
    vi.useFakeTimers()
    const { scanner, wrapper } = await startLive()

    mocks.detectCardQuadCv.mockReturnValueOnce(quadAt(0.2, 0.15)).mockReturnValue(null)
    vi.advanceTimersByTime(120)
    expect(scanner.detectedQuad.value).not.toBeNull()

    scanner.stop()
    expect(scanner.detectedQuad.value).toBeNull()

    // Restart: the first tick misses — a tracker that survived stop() would still
    // hold the pre-stop quad here.
    await scanner.start()
    await flushPromises()
    vi.advanceTimersByTime(120)
    expect(scanner.detectedQuad.value).toBeNull()

    wrapper.unmount()
  })
})
