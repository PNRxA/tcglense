import { defineComponent, ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'

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
  loadOpenCv: vi.fn<() => Promise<never>>(() => new Promise<never>(() => {})),
  detectCardQuadCv: vi.fn<() => null>(() => null),
}))

import { useCardScanner } from '../useCardScanner'

function mountScanner() {
  let scanner!: ReturnType<typeof useCardScanner>
  const wrapper = mount(
    defineComponent({
      setup() {
        scanner = useCardScanner(ref(null))
        return () => null
      },
    }),
  )
  return { scanner, wrapper }
}

afterEach(() => {
  vi.clearAllMocks()
  Reflect.deleteProperty(navigator, 'mediaDevices')
})

describe('useCardScanner OCR worker', () => {
  it('creates the tesseract worker from self-hosted same-origin assets, never a CDN', async () => {
    const fakeStream = { getTracks: () => [], getVideoTracks: () => [] }
    Object.defineProperty(navigator, 'mediaDevices', {
      configurable: true,
      value: {
        getUserMedia: vi.fn<() => Promise<typeof fakeStream>>(async () => fakeStream),
      },
    })
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
