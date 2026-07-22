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
      (
        cv: unknown,
        image: { width: number; height: number },
        opts: {
          fallbackPasses: boolean
          window?: { x: number; y: number; width: number; height: number }
          select?: { mode: string; prior?: unknown; guide?: unknown }
        },
      ) => Quad | null
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
  vi.restoreAllMocks()
  vi.clearAllMocks()
  mocks.loadOpenCv.mockImplementation(() => new Promise<never>(() => {}))
  mocks.detectCardQuadCv.mockReturnValue(null)
  mocks.hamming.mockReturnValue(0)
  vi.useRealTimers()
  vi.unstubAllGlobals()
  Reflect.deleteProperty(navigator, 'mediaDevices')
})

describe('useCardScanner OpenCV warm-up', () => {
  it('starts loading the full detector as soon as the scanner page mounts', () => {
    const { scanner, wrapper } = mountScanner()

    expect(mocks.loadOpenCv).toHaveBeenCalledTimes(1)
    expect(scanner.cvStatus.value).toBe('loading')

    wrapper.unmount()
  })

  it('reports an explicit fallback after failure and retries when the camera starts', async () => {
    mockCamera()
    const failure = new Error('chunk unavailable')
    mocks.loadOpenCv.mockRejectedValueOnce(failure)
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const { scanner, wrapper } = mountScanner()

    await flushPromises()
    expect(scanner.cvStatus.value).toBe('fallback')
    expect(warn).toHaveBeenCalledWith('OpenCV failed to load; using basic card detection', failure)

    await scanner.start()
    await flushPromises()
    expect(mocks.loadOpenCv).toHaveBeenCalledTimes(2)
    expect(scanner.cvStatus.value).toBe('loading')

    wrapper.unmount()
  })
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
    const getContext = vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({
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
    return { video: ref(el as unknown as HTMLVideoElement), getContext }
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
    const { video } = mockVideoAndCanvas()
    const { scanner, wrapper } = mountScanner(video)
    await scanner.start()
    await flushPromises() // lets the mocked OpenCV runtime finish "loading"
    return { scanner, wrapper }
  }

  it('waits for OpenCV instead of running the weak detector during warm-up', async () => {
    vi.useFakeTimers()
    mockCamera()
    const { video, getContext } = mockVideoAndCanvas()
    const { scanner, wrapper } = mountScanner(video)
    await scanner.start()

    vi.advanceTimersByTime(360)

    expect(scanner.cvStatus.value).toBe('loading')
    expect(getContext).not.toHaveBeenCalled()
    expect(scanner.detectedQuad.value).toBeNull()

    wrapper.unmount()
  })

  it('shows no outline for a single detection, locks on the confirming second tick', async () => {
    vi.useFakeTimers()
    const { scanner, wrapper } = await startLive()

    const first = quadAt(0.2, 0.15)
    const jittered = quadAt(0.22, 0.15)
    mocks.detectCardQuadCv
      .mockReturnValueOnce(first)
      .mockReturnValueOnce(jittered)
      .mockReturnValue(null)

    // One isolated detection is tentative: a transient clutter quad must never flash
    // a green lock the user could try to capture.
    vi.advanceTimersByTime(120)
    expect(scanner.detectedQuad.value).toBeNull()

    // The consistent second detection confirms — blended from the tentative quad
    // (default 0.5), so the outline appears already settled at the midpoint.
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

  it('runs cardless full-frame acquisition with fallback passes on a 1-in-3 cadence', async () => {
    vi.useFakeTimers()
    const { wrapper } = await startLive()

    mocks.detectCardQuadCv.mockReturnValue(null)
    for (let tick = 0; tick < 7; tick++) vi.advanceTimersByTime(120)

    // Cardless ticks search the full frame in acquisition mode; the expensive
    // fallback passes only run every third consecutive miss.
    const calls = mocks.detectCardQuadCv.mock.calls
    expect(calls.map((call) => call[2].fallbackPasses)).toEqual([
      true,
      false,
      false,
      true,
      false,
      false,
      true,
    ])
    for (const call of calls) {
      expect(call[2].select!.mode).toBe('acquisition')
      expect(call[2].window).toBeUndefined()
    }

    wrapper.unmount()
  })

  it('searches a prior ROI in tracking mode once a card is seen, full-frame on the 2nd miss', async () => {
    vi.useFakeTimers()
    const { wrapper } = await startLive()

    const seen = quadAt(0.2, 0.15)
    mocks.detectCardQuadCv.mockReturnValueOnce(seen).mockReturnValueOnce(seen).mockReturnValue(null)

    // Tick 1: cold acquisition finds the card (tentative). Ticks 2-3: the tentative /
    // locked prior narrows the search to its padded ROI, always with the full ladder.
    for (let tick = 0; tick < 3; tick++) vi.advanceTimersByTime(120)
    const calls = mocks.detectCardQuadCv.mock.calls
    expect(calls[0]![2].select!.mode).toBe('acquisition')
    // The association prior is the ACTUAL tentative/displayed quad — not just any
    // value; a wrong prior would make the real detector reject the card itself.
    for (const call of calls.slice(1)) {
      expect(call[2].select!).toEqual({ mode: 'tracking', prior: seen })
    }
    expect(calls[1]![2].window).toMatchObject({ fullWidth: 640, fullHeight: 480 })
    expect(calls[1]![2].fallbackPasses).toBe(true)
    // The ROI read is the window's crop, not the whole frame.
    expect(calls[1]![1]).toMatchObject({
      width: calls[1]![2].window!.width,
      height: calls[1]![2].window!.height,
    })

    // Tick 3 was the first ROI miss (tolerated, single call). Tick 4 = second
    // consecutive miss: the ROI search is retried full-frame, still prior-associated,
    // but WITHOUT the fallback ladder — the retry must not defeat the cadence-gated
    // cost bound.
    const tick4Before = mocks.detectCardQuadCv.mock.calls.length
    vi.advanceTimersByTime(120)
    const tick4Calls = mocks.detectCardQuadCv.mock.calls.slice(tick4Before)
    expect(tick4Calls).toHaveLength(2)
    expect(tick4Calls[1]![2].window).toBeUndefined()
    expect(tick4Calls[1]![2].select!.mode).toBe('tracking')
    expect(tick4Calls[1]![2].fallbackPasses).toBe(false)

    wrapper.unmount()
  })

  it('falls back to a full-frame capture detect when the ROI search misses', async () => {
    vi.useFakeTimers()
    mockBurstFrameEnvironment()
    const { scanner, wrapper } = await startLive()
    const locked = quadAt(0.2, 0.15)
    mocks.detectCardQuadCv.mockReturnValueOnce(locked).mockReturnValueOnce(locked)
    vi.advanceTimersByTime(240)
    expect(scanner.detectedQuad.value).toEqual(locked)

    // First capture frame: the ROI search misses (e.g. the card sits against the
    // window's artificial edge) and the full-frame retry recovers it.
    mocks.detectCardQuadCv.mockReturnValueOnce(null).mockReturnValue(locked)
    const captured = await scanner.capture()
    expect(captured).not.toBeNull()

    const captureCalls = mocks.detectCardQuadCv.mock.calls.slice(2)
    expect(captureCalls[0]![2].window).toBeDefined()
    expect(captureCalls[0]![2].select!).toEqual({ mode: 'capture', prior: locked })
    expect(captureCalls[1]![2].window).toBeUndefined()
    expect(captureCalls[1]![2].select!).toEqual({ mode: 'capture', prior: locked })

    wrapper.unmount()
  })

  it('refuses capture when fresh full-resolution geometry no longer matches the lock', async () => {
    vi.useFakeTimers()
    const { scanner, wrapper } = await startLive()
    const locked = quadAt(0.2, 0.15)
    const oneCornerAtEdge: Quad = locked.map((point) => ({ ...point })) as Quad
    oneCornerAtEdge[0] = { x: 0, y: 0 }

    mocks.detectCardQuadCv
      .mockReturnValueOnce(locked)
      .mockReturnValueOnce(locked)
      .mockReturnValueOnce(oneCornerAtEdge)
    vi.advanceTimersByTime(240)
    expect(scanner.detectedQuad.value).toEqual(locked)

    expect(await scanner.capture()).toBeNull()
    expect(scanner.detectedQuad.value).toBeNull()
    // The capture re-detection searches the tracked prior's ROI in capture mode with
    // the full detection ladder.
    expect(mocks.detectCardQuadCv).toHaveBeenLastCalledWith(
      expect.anything(),
      expect.anything(),
      expect.objectContaining({
        fallbackPasses: true,
        select: { mode: 'capture', prior: locked },
        window: expect.objectContaining({ fullWidth: 640, fullHeight: 480 }),
      }),
    )
    // Strong capture-time evidence invalidates the stale green hold immediately; the
    // next two good live frames reacquire without waiting out the old miss window.
    mocks.detectCardQuadCv.mockReturnValue(locked)
    vi.advanceTimersByTime(240)
    expect(scanner.detectedQuad.value).toEqual(locked)

    wrapper.unmount()
  })

  it('rejects a coherent inner-frame crop that would omit the card border', async () => {
    vi.useFakeTimers()
    const { scanner, wrapper } = await startLive()
    const outer = quadAt(0.2, 0.1, 0.6, 0.8)
    const inner = quadAt(0.224, 0.132, 0.552, 0.736)

    mocks.detectCardQuadCv
      .mockReturnValueOnce(outer)
      .mockReturnValueOnce(outer)
      .mockReturnValueOnce(inner)
    vi.advanceTimersByTime(240)

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
    vi.advanceTimersByTime(240)
    const callsBeforeCapture = mocks.detectCardQuadCv.mock.calls.length

    const captured = await scanner.capture()

    expect(captured).not.toBeNull()
    expect(captured!.fingerprints).toHaveLength(36)
    // Every pooled frame has fresh high-resolution geometry; the live tracked quad is
    // only the initial reference and cannot mutate the in-progress burst.
    expect(mocks.detectCardQuadCv).toHaveBeenCalledTimes(callsBeforeCapture + 4)
    // Burst frames search the previous accepted quad's ROI in capture mode.
    expect(mocks.detectCardQuadCv).toHaveBeenLastCalledWith(
      expect.anything(),
      expect.anything(),
      expect.objectContaining({
        fallbackPasses: true,
        select: expect.objectContaining({ mode: 'capture' }),
        window: expect.anything(),
      }),
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
      .mockReturnValueOnce(locked) // live: tentative
      .mockReturnValueOnce(locked) // live: confirm → lock
      .mockReturnValueOnce(locked) // first capture frame
      .mockReturnValueOnce(moved) // next frame: stop the burst
    vi.advanceTimersByTime(240)
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
    vi.advanceTimersByTime(240)

    const captured = await scanner.capture()

    expect(captured).not.toBeNull()
    expect(captured!.fingerprints).toHaveLength(9)
    expect(mocks.hamming).toHaveBeenCalledTimes(1)
    wrapper.unmount()
  })

  it('stop() forgets the track — no stale outline can survive a restart', async () => {
    vi.useFakeTimers()
    const { scanner, wrapper } = await startLive()

    mocks.detectCardQuadCv
      .mockReturnValueOnce(quadAt(0.2, 0.15))
      .mockReturnValueOnce(quadAt(0.2, 0.15))
      .mockReturnValue(null)
    vi.advanceTimersByTime(240)
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
