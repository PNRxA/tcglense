import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { enableAutoUnmount, flushPromises, shallowMount } from '@vue/test-utils'
import type { Ref } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import ScanView from '@/views/ScanView.vue'
import ScanCameraSurface from '@/components/collection/ScanCameraSurface.vue'
import ScanCaptureDock from '@/components/collection/ScanCaptureDock.vue'
import ScanMatchPanel from '@/components/collection/ScanMatchPanel.vue'
import { useScanPreferencesStore } from '@/stores/scanPreferences'

// Typed mock helpers (the repo's lint requires a type parameter on vi.fn()).
const voidFn = () => vi.fn<() => void>()

// The scanner + session composables are mocked to controllable refs so the test drives the
// auto-scroll decision directly, without a real camera / OpenCV / network. Each field ScanView
// reads is a shared ref exposed on `scanner` / `session` so a test can mutate `.value`.
const H = vi.hoisted(() => ({
  scanner: {} as Record<string, unknown>,
  session: {} as Record<string, unknown>,
  capture: vi.fn<() => Promise<unknown>>(),
  handleCapture: vi.fn<() => Promise<string>>(),
  confirmCurrent: vi.fn<() => Promise<void>>(),
  discardCurrent: vi.fn<() => void>(),
  finalizeCurrent: vi.fn<() => Promise<boolean>>(),
}))

vi.mock('@/lib/seo', () => ({ usePageMeta: () => {} }))
vi.mock('vue-router', () => ({ onBeforeRouteLeave: () => {} }))

vi.mock('@/composables/useCardScanner', async () => {
  const { ref } = await import('vue')
  H.scanner.status = ref('ready')
  H.scanner.errorMessage = ref(null)
  H.scanner.ocrLoading = ref(false)
  H.scanner.cvStatus = ref('ready')
  H.scanner.interrupted = ref(false)
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
      finalizeCurrent: H.finalizeCurrent,
      confirmCurrent: H.confirmCurrent,
      discardCurrent: H.discardCurrent,
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

// The session's current match, so a test can put a card on screen and let the mocked
// confirm/discard clear it the way the real session does.
const matchRef = () => H.session.match as Ref<unknown>
const A_MATCH = { ocrName: 'Sol Ring', hint: {}, candidates: ['Sol Ring'], name: 'Sol Ring' }

// jsdom implements neither of these; stub them so reviewMatch() can call them and be asserted.
const scrollIntoView = vi.fn<() => void>()
const focus = vi.fn<() => void>()
let isDesktop = false

beforeEach(() => {
  // The scan preferences are persisted, so a test that flips the toggle would otherwise
  // leak that setting into every test after it.
  localStorage.clear()
  setActivePinia(createPinia())
  isDesktop = false
  scrollIntoView.mockClear()
  focus.mockClear()
  H.capture.mockReset()
  H.capture.mockResolvedValue({ fingerprints: [1], setText: '', foil: false })
  H.handleCapture.mockReset()
  H.handleCapture.mockResolvedValue('matched')
  matchRef().value = null
  // The real confirm/discard clear the panel; a failed save is the override a test sets.
  H.confirmCurrent.mockReset()
  H.confirmCurrent.mockImplementation(async () => {
    matchRef().value = null
  })
  H.discardCurrent.mockReset()
  H.discardCurrent.mockImplementation(() => {
    matchRef().value = null
  })
  H.finalizeCurrent.mockReset()
  H.finalizeCurrent.mockResolvedValue(true)
  Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' })
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

// The view now listens on `document`/`window` (see onPageHidden), so a wrapper left mounted
// would keep reacting to the next test's events.
enableAutoUnmount(afterEach)

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

describe('ScanView leaving the page', () => {
  /** jsdom's visibilityState is read-only, so drive it through the prototype like the browser. */
  function hidePage() {
    Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' })
    document.dispatchEvent(new Event('visibilitychange'))
  }

  it('saves the tentative card and clears the panel when the page is hidden', async () => {
    matchRef().value = A_MATCH
    shallowMount(ScanView)

    hidePage()
    await flushPromises()

    // A backgrounded mobile tab can be discarded without ever unmounting, so hiding is the
    // last moment the card on screen can be written at all.
    expect(H.finalizeCurrent).toHaveBeenCalledOnce()
    expect(H.discardCurrent).toHaveBeenCalledOnce()
  })

  it('clears only the card it saved, never one a capture swapped in mid-save', async () => {
    // The one path that can finalize with a capture in flight: the user taps Scan and leaves
    // before the match request answers. That capture's handleCapture commits the settled card
    // and puts a NEW match on screen while this save settles — clearing "whatever is there
    // now" would silently drop a card that was scanned but never written.
    const nextMatch = { ...A_MATCH, name: 'Llanowar Elves' }
    matchRef().value = A_MATCH
    H.finalizeCurrent.mockImplementation(async () => {
      matchRef().value = nextMatch
      return true
    })
    shallowMount(ScanView)

    hidePage()
    await flushPromises()

    expect(H.discardCurrent).not.toHaveBeenCalled()
    expect(matchRef().value).toStrictEqual(nextMatch)
  })

  it('keeps the match on screen when that save failed, so it can be retried', async () => {
    H.finalizeCurrent.mockResolvedValue(false)
    matchRef().value = A_MATCH
    shallowMount(ScanView)

    hidePage()
    await flushPromises()

    expect(H.finalizeCurrent).toHaveBeenCalledOnce()
    expect(H.discardCurrent).not.toHaveBeenCalled()
    expect(matchRef().value).toStrictEqual(A_MATCH)
  })
})

describe('ScanView scroll back to the camera', () => {
  async function mountWithMatch(event: 'confirm' | 'discard') {
    matchRef().value = A_MATCH
    const wrapper = shallowMount(ScanView)
    await wrapper.findComponent(ScanMatchPanel).vm.$emit(event)
    await flushPromises()
    return wrapper
  }

  it('scrolls back after Add card, without stealing focus', async () => {
    await mountWithMatch('confirm')
    expect(H.confirmCurrent).toHaveBeenCalledOnce()
    expect(scrollIntoView).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ block: 'start' }),
    )
    expect(focus).not.toHaveBeenCalled()
  })

  it('scrolls back after Discard', async () => {
    await mountWithMatch('discard')
    expect(H.discardCurrent).toHaveBeenCalledOnce()
    expect(scrollIntoView).toHaveBeenCalledOnce()
  })

  it('stays put when the save failed and the match is still on screen to retry', async () => {
    H.confirmCurrent.mockImplementation(async () => {})
    await mountWithMatch('confirm')
    expect(H.confirmCurrent).toHaveBeenCalledOnce()
    expect(scrollIntoView).not.toHaveBeenCalled()
  })

  it('does not scroll on the two-column (lg+) layout, where the camera is already visible', async () => {
    isDesktop = true
    await mountWithMatch('confirm')
    expect(scrollIntoView).not.toHaveBeenCalled()
  })

  it('does not scroll when the toggle is off', async () => {
    useScanPreferencesStore().setAutoScrollToReview(false)
    await mountWithMatch('confirm')
    expect(scrollIntoView).not.toHaveBeenCalled()
  })
})
