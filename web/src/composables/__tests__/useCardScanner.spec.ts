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
      getImageData: (x: number, y: number, w: number, h: number) => ({
        data: new Uint8ClampedArray(w * h * 4),
        width: w,
        height: h,
      }),
    } as unknown as CanvasRenderingContext2D)
    return ref(el as unknown as HTMLVideoElement)
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
