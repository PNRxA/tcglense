import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, shallowMount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import ScanView from '@/views/ScanView.vue'
import ScanCameraSurface from '@/components/collection/ScanCameraSurface.vue'
import ScanCaptureDock from '@/components/collection/ScanCaptureDock.vue'
import { useScanPreferencesStore } from '@/stores/scanPreferences'

// Typed mock helpers (the repo's lint requires a type parameter on vi.fn()).
const voidFn = () => vi.fn<() => void>()
const asyncTrue = () => vi.fn<() => Promise<boolean>>(async () => true)

// The scanner + session composables are mocked to controllable refs so the test drives the
// auto-scroll decision directly, without a real camera / OpenCV / network. Each field ScanView
// reads is a shared ref exposed on `scanner` / `session` so a test can mutate `.value`.
const H = vi.hoisted(() => ({
  scanner: {} as Record<string, unknown>,
  session: {} as Record<string, unknown>,
  capture: vi.fn<() => Promise<unknown>>(),
  handleCapture: vi.fn<() => Promise<string>>(),
}))

vi.mock('@/lib/seo', () => ({ usePageMeta: () => {} }))
vi.mock('vue-router', () => ({ onBeforeRouteLeave: () => {} }))

vi.mock('@/composables/useCardScanner', async () => {
  const { ref } = await import('vue')
  H.scanner.status = ref('ready')
  H.scanner.errorMessage = ref(null)
  H.scanner.ocrLoading = ref(false)
  H.scanner.cvStatus = ref('ready')
  H.scanner.detectedQuad = ref({
    a: { x: 0, y: 0 },
    b: { x: 1, y: 0 },
    c: { x: 1, y: 1 },
    d: { x: 0, y: 1 },
  })
  return {
    useCardScanner: () => ({
      ...H.scanner,
      start: voidFn(),
      stop: voidFn(),
      switchCamera: voidFn(),
      capture: H.capture,
    }),
  }
})

vi.mock('@/composables/useScanSession', async () => {
  const { ref } = await import('vue')
  const s = H.session
  s.match = ref(null)
  s.prints = ref([])
  s.printsFilter = ref('')
  s.printsFiltered = ref([])
  s.printsLoading = ref(false)
  s.printsLoadingMore = ref(false)
  s.printsError = ref(false)
  s.printsTotal = ref(0)
  s.printsHasMore = ref(false)
  s.selectedId = ref('')
  s.selectedCard = ref(null)
  s.owned = ref({ quantity: 0, foil_quantity: 0 })
  s.target = ref({ quantity: 0, foil_quantity: 0 })
  s.ready = ref(false)
  s.advanceReady = ref(true)
  s.resolving = ref(false)
  s.finalizing = ref(false)
  s.undoing = ref(false)
  s.ownedError = ref(false)
  s.candidates = ref([])
  s.log = ref([])
  s.addedCount = ref(0)
  s.unrecognized = ref(false)
  s.commitError = ref(false)
  return {
    useScanSession: () => ({
      ...s,
      handleCapture: H.handleCapture,
      finalizeCurrent: asyncTrue(),
      confirmCurrent: voidFn(),
      discardCurrent: voidFn(),
      selectId: voidFn(),
      setName: voidFn(),
      adjust: voidFn(),
      undo: voidFn(),
      retryOwned: voidFn(),
      retryPrintings: voidFn(),
      pickCandidate: voidFn(),
      loadMorePrintings: voidFn(),
    }),
  }
})

// jsdom implements neither of these; stub them so reviewMatch() can call them and be asserted.
const scrollIntoView = vi.fn<() => void>()
const focus = vi.fn<() => void>()
let isDesktop = false

beforeEach(() => {
  setActivePinia(createPinia())
  isDesktop = false
  scrollIntoView.mockClear()
  focus.mockClear()
  H.capture.mockReset()
  H.capture.mockResolvedValue({ fingerprints: [1], setText: '', foil: false })
  H.handleCapture.mockReset()
  H.handleCapture.mockResolvedValue('matched')
  Element.prototype.scrollIntoView = scrollIntoView
  HTMLElement.prototype.focus = focus
  window.matchMedia = vi.fn<(query: string) => MediaQueryList>().mockImplementation(
    (query: string) =>
      ({
        matches: query.includes('min-width: 1024px') ? isDesktop : false,
        media: query,
        onchange: null,
        addEventListener: voidFn(),
        removeEventListener: voidFn(),
        addListener: voidFn(),
        removeListener: voidFn(),
        dispatchEvent: vi.fn<() => boolean>(),
      }) as unknown as MediaQueryList,
  )
})

afterEach(() => {
  vi.clearAllMocks()
})

async function mountAndCapture() {
  const wrapper = shallowMount(ScanView)
  await wrapper.findComponent(ScanCameraSurface).vm.$emit('capture')
  await flushPromises()
  return wrapper
}

describe('ScanView auto-scroll to review', () => {
  it('scrolls the review into view after a fresh match, without stealing focus (default on)', async () => {
    await mountAndCapture()
    expect(H.handleCapture).toHaveBeenCalledOnce()
    expect(scrollIntoView).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ block: 'start' }),
    )
    // The automatic scroll must not move keyboard focus (only the manual "Review" tap does).
    expect(focus).not.toHaveBeenCalled()
  })

  it('does not scroll when the toggle is off', async () => {
    useScanPreferencesStore().setAutoScrollToReview(false)
    await mountAndCapture()
    expect(H.handleCapture).toHaveBeenCalledOnce()
    expect(scrollIntoView).not.toHaveBeenCalled()
  })

  it('does not scroll on the two-column (lg+) layout, where the review is already visible', async () => {
    isDesktop = true
    await mountAndCapture()
    expect(H.handleCapture).toHaveBeenCalledOnce()
    expect(scrollIntoView).not.toHaveBeenCalled()
  })

  it('does not scroll when the capture was not a fresh match (same / unmatched / busy)', async () => {
    H.handleCapture.mockResolvedValue('same')
    await mountAndCapture()
    expect(H.handleCapture).toHaveBeenCalledOnce()
    expect(scrollIntoView).not.toHaveBeenCalled()
  })

  it('the manual Review control scrolls and does move focus', async () => {
    const wrapper = shallowMount(ScanView)
    await wrapper.findComponent(ScanCaptureDock).vm.$emit('review')
    await flushPromises()
    expect(scrollIntoView).toHaveBeenCalledOnce()
    expect(focus).toHaveBeenCalledOnce()
  })
})
