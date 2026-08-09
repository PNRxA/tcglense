import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest'
import { defineComponent, ref, type Ref } from 'vue'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { useScanSession } from '@/composables/useScanSession'
import { makeCard } from '@/test/fixtures'
import type { Card, CollectionQuantities } from '@/lib/api'

const mocks = vi.hoisted(() => ({
  usePrintingPicker: vi.fn<(...args: unknown[]) => unknown>(),
  useCollectionEntryQuery: vi.fn<(...args: unknown[]) => unknown>(),
  useSetCollectionEntryMutation: vi.fn<(...args: unknown[]) => unknown>(),
  useScanMutation: vi.fn<(...args: unknown[]) => unknown>(),
}))

vi.mock('@/composables/usePrintings', () => ({
  usePrintingPicker: mocks.usePrintingPicker,
}))

vi.mock('@/composables/useCollection', () => ({
  useCollectionEntryQuery: mocks.useCollectionEntryQuery,
  useSetCollectionEntryMutation: mocks.useSetCollectionEntryMutation,
}))

vi.mock('@/composables/useScan', () => ({
  useScanMutation: mocks.useScanMutation,
}))

interface PickerState {
  printings: Ref<Card[]>
  total: Ref<number>
  hasNextPage: Ref<boolean>
  isPending: Ref<boolean>
  isFetching: Ref<boolean>
  isFetchingNextPage: Ref<boolean>
  failed: Ref<boolean>
  loadMore: Mock<() => Promise<void>>
  refetch: Mock<() => Promise<void>>
}

interface SaveInput {
  game: string
  id: string
  quantity: number
  foil_quantity: number
}

function deferred() {
  let resolve!: () => void
  const promise = new Promise<void>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

let wrapper: VueWrapper | undefined
let picker: PickerState
let session: ReturnType<typeof useScanSession>
let save: Mock<(input: SaveInput) => Promise<void>>
let ownedData: Ref<CollectionQuantities>

function persistCounts(input: SaveInput) {
  ownedData.value = {
    quantity: input.quantity,
    foil_quantity: input.foil_quantity,
  }
}

async function mountSession() {
  const visualMatch = makeCard('visual-match')
  mocks.useScanMutation.mockReturnValue({
    mutateAsync: vi
      .fn<(...args: unknown[]) => Promise<{ data: Array<{ card: Card; distance: number }> }>>()
      .mockResolvedValue({ data: [{ card: visualMatch, distance: 0 }] }),
  })

  const Host = defineComponent({
    setup() {
      session = useScanSession(ref('mtg'))
      return () => null
    },
  })
  wrapper = mount(Host)

  await session.handleCapture({
    fingerprints: [new Uint8Array(32)],
    setText: 'OLD • EN',
    foil: false,
  })
  await flushPromises()
}

beforeEach(() => {
  const newest = makeCard('newest', { set_code: 'new' })
  picker = {
    printings: ref([newest]),
    total: ref(201),
    hasNextPage: ref(true),
    isPending: ref(false),
    isFetching: ref(false),
    isFetchingNextPage: ref(false),
    failed: ref(false),
    loadMore: vi.fn<() => Promise<void>>(),
    refetch: vi.fn<() => Promise<void>>(),
  }
  mocks.usePrintingPicker.mockReturnValue(picker)
  ownedData = ref({ quantity: 0, foil_quantity: 0 })
  mocks.useCollectionEntryQuery.mockReturnValue({
    data: ownedData,
    isSuccess: ref(true),
    isFetching: ref(false),
    isError: ref(false),
    refetch: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
  })
  save = vi.fn<(input: SaveInput) => Promise<void>>(async (input) => persistCounts(input))
  mocks.useSetCollectionEntryMutation.mockReturnValue({
    isPending: ref(false),
    mutateAsync: save,
  })
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
  vi.clearAllMocks()
})

describe('useScanSession printing resolution', () => {
  it('keeps a hinted printing unresolved after pagination fails and allows retry', async () => {
    const firstPage = deferred()
    const target = makeCard('old-printing', { set_code: 'old' })
    picker.loadMore
      .mockImplementationOnce(async () => {
        picker.isFetching.value = true
        picker.isFetchingNextPage.value = true
        await firstPage.promise
        picker.failed.value = true
        picker.isFetching.value = false
        picker.isFetchingNextPage.value = false
      })
      .mockImplementationOnce(async () => {
        picker.isFetching.value = true
        picker.isFetchingNextPage.value = true
        picker.printings.value = [...picker.printings.value, target]
        picker.hasNextPage.value = false
        picker.failed.value = false
        picker.isFetching.value = false
        picker.isFetchingNextPage.value = false
      })

    await mountSession()
    expect(picker.loadMore).toHaveBeenCalledTimes(1)

    const finalizing = session.finalizeCurrent()
    firstPage.resolve()
    await flushPromises()

    await expect(finalizing).resolves.toBe(false)
    expect(session.printsError.value).toBe(true)
    expect(session.selectedId.value).toBe('')
    expect(save).not.toHaveBeenCalled()

    session.retryPrintings()
    await flushPromises()

    expect(picker.loadMore).toHaveBeenCalledTimes(2)
    expect(session.selectedId.value).toBe(target.id)
  })

  it('waits for multi-page resolution before committing the final card', async () => {
    const nextPage = deferred()
    const target = makeCard('old-printing', { set_code: 'old' })
    picker.loadMore.mockImplementation(async () => {
      picker.isFetching.value = true
      picker.isFetchingNextPage.value = true
      await nextPage.promise
      picker.printings.value = [...picker.printings.value, target]
      picker.hasNextPage.value = false
      picker.isFetching.value = false
      picker.isFetchingNextPage.value = false
    })

    await mountSession()
    const finalizing = session.finalizeCurrent()
    let finished = false
    void finalizing.then(() => {
      finished = true
    })
    await flushPromises()

    expect(session.finalizing.value).toBe(true)
    expect(finished).toBe(false)
    expect(save).not.toHaveBeenCalled()

    nextPage.resolve()
    await expect(finalizing).resolves.toBe(true)

    expect(session.selectedId.value).toBe(target.id)
    expect(save).toHaveBeenCalledWith({
      game: 'mtg',
      id: target.id,
      quantity: 1,
      foil_quantity: 0,
    })
    expect(session.finalizing.value).toBe(false)
  })

  it('freezes capture and match edits while the final save is pending', async () => {
    const savePending = deferred()
    const target = makeCard('old-printing', { set_code: 'old' })
    const alternate = makeCard('alternate', { set_code: 'alt' })
    picker.printings.value = [target, alternate]
    picker.hasNextPage.value = false
    save.mockImplementation(() => savePending.promise)

    await mountSession()
    expect(session.selectedId.value).toBe(target.id)
    expect(session.target.quantity).toBe(1)

    const finalizing = session.finalizeCurrent()
    expect(session.finalizing.value).toBe(true)

    session.adjust('quantity', 1)
    session.selectId(alternate.id)
    session.discardCurrent()
    await expect(
      session.handleCapture({
        fingerprints: [new Uint8Array(32)],
        setText: 'ALT • EN',
        foil: false,
      }),
    ).resolves.toBe('busy')

    expect(session.target.quantity).toBe(1)
    expect(session.selectedId.value).toBe(target.id)
    expect(session.match.value).not.toBeNull()

    savePending.resolve()
    await expect(finalizing).resolves.toBe(true)
    expect(save).toHaveBeenCalledTimes(1)
  })

  it('serializes a session undo before the final card save', async () => {
    const undoPending = deferred()
    const target = makeCard('old-printing', { set_code: 'old' })
    picker.printings.value = [target]
    picker.hasNextPage.value = false

    await mountSession()
    await session.confirmCurrent()
    expect(session.log.value).toHaveLength(1)

    await session.handleCapture({
      fingerprints: [new Uint8Array(32)],
      setText: 'OLD • EN',
      foil: false,
    })
    await flushPromises()
    expect(session.ready.value).toBe(true)
    expect(session.target.quantity).toBe(2)

    save.mockClear()
    save
      .mockImplementationOnce(async (input) => {
        await undoPending.promise
        persistCounts(input)
      })
      .mockImplementation(async (input) => persistCounts(input))
    const undoing = session.undo(0)
    expect(session.undoing.value).toBe(true)

    const finalizing = session.finalizeCurrent()
    expect(session.finalizing.value).toBe(true)
    expect(save).toHaveBeenCalledTimes(1)

    undoPending.resolve()
    await expect(undoing).resolves.toBe(true)
    await expect(finalizing).resolves.toBe(true)

    expect(save.mock.calls).toEqual([
      [
        {
          game: 'mtg',
          id: target.id,
          quantity: 0,
          foil_quantity: 0,
        },
      ],
      [
        {
          game: 'mtg',
          id: target.id,
          quantity: 1,
          foil_quantity: 0,
        },
      ],
    ])
  })
})

describe('useScanSession visual printing priority', () => {
  // Two treatments of one card in one set: the listing can't order them meaningfully (same
  // name, same release date), so only the fingerprint ranking says which one was scanned.
  const fullArt = makeCard('tla-fullart', { set_code: 'tla', collector_number: '312' })
  const normal = makeCard('tla-normal', { set_code: 'tla', collector_number: '41' })

  // `setText` is the raw OCR of the bottom strip: 'TLA • EN' yields a set-code hint, '' yields
  // none. The pagination path has to be exercised with no set code, or the pre-existing
  // set-code branch drives the loading and the visual wait is never actually tested.
  async function captureRanked(
    ranked: Array<{ card: Card; distance: number }>,
    { hasMore = false, setText = 'TLA • EN' } = {},
  ) {
    // Listing order puts the full-art treatment first, which used to become the pick.
    picker.printings.value = [fullArt, normal]
    picker.hasNextPage.value = hasMore
    mocks.useScanMutation.mockReturnValue({
      // A fresh array per call, as the real endpoint gives — a shared one would make a
      // rescan a no-op for anything watching `candidates`.
      mutateAsync: vi
        .fn<(...args: unknown[]) => Promise<{ data: Array<{ card: Card; distance: number }> }>>()
        .mockImplementation(async () => ({ data: ranked.map((m) => ({ ...m })) })),
    })

    const Host = defineComponent({
      setup() {
        session = useScanSession(ref('mtg'))
        return () => null
      },
    })
    wrapper = mount(Host)
    await session.handleCapture({ fingerprints: [new Uint8Array(32)], setText, foil: false })
    await flushPromises()
  }

  it('opens on the visually closest treatment, not the first of the hinted set', async () => {
    // The set line reads cleanly but only names the set — every treatment shares it.
    await captureRanked([
      { card: normal, distance: 14 },
      { card: fullArt, distance: 89 },
    ])
    expect(session.selectedId.value).toBe(normal.id)
  })

  it('paginates for the closest ranked printing even with no set code to go on', async () => {
    // Nothing readable on the strip, so only `awaitingRankedPrinting` can drive the load —
    // without it the pick would settle for the best of page one.
    const later = makeCard('tla-later', { set_code: 'tla', collector_number: '7' })
    picker.loadMore.mockImplementation(async () => {
      picker.printings.value = [...picker.printings.value, later]
      picker.hasNextPage.value = false
    })
    await captureRanked(
      [
        { card: later, distance: 11 },
        { card: fullArt, distance: 84 },
      ],
      { hasMore: true, setText: '' },
    )
    await flushPromises()

    expect(picker.loadMore).toHaveBeenCalled()
    expect(session.selectedId.value).toBe(later.id)
  })

  it('keeps a candidate picked from the strip when the same card is rescanned', async () => {
    // A strip pick can name a printing that isn't in the loaded pages. A rescan reassigns
    // `candidates`, which re-runs the auto-pick — it must not quietly overwrite the choice.
    const unloaded = makeCard('tla-unloaded', { set_code: 'tla', collector_number: '99' })
    await captureRanked([
      { card: normal, distance: 14 },
      { card: fullArt, distance: 89 },
    ])
    expect(session.selectedId.value).toBe(normal.id)

    session.pickCandidate(unloaded)
    await flushPromises()
    expect(session.selectedId.value).toBe(unloaded.id)

    await session.handleCapture({
      fingerprints: [new Uint8Array(32)],
      setText: 'TLA • EN',
      foil: false,
    })
    await flushPromises()
    expect(session.selectedId.value).toBe(unloaded.id)
  })
})

describe('useScanSession foil detection', () => {
  // Mount a session and feed one capture, resolving to a single already-loaded printing. `foil`
  // is the scanner's visual foil-star verdict (see lib/scan/foilStar).
  async function captureInto(foil: boolean) {
    const target = makeCard('star-printing', { set_code: 'neo' })
    picker.printings.value = [target]
    picker.hasNextPage.value = false

    const visualMatch = makeCard('visual-match')
    mocks.useScanMutation.mockReturnValue({
      mutateAsync: vi
        .fn<(...args: unknown[]) => Promise<{ data: Array<{ card: Card; distance: number }> }>>()
        .mockResolvedValue({ data: [{ card: visualMatch, distance: 0 }] }),
    })

    const Host = defineComponent({
      setup() {
        session = useScanSession(ref('mtg'))
        return () => null
      },
    })
    wrapper = mount(Host)
    await session.handleCapture({ fingerprints: [new Uint8Array(32)], setText: 'NEO • EN', foil })
    await flushPromises()
    return target
  }

  it('seeds the scanned copy as foil when the capture detected a foil star', async () => {
    const target = await captureInto(true)
    expect(session.ready.value).toBe(true)
    // The star routes the +1 into the foil count; the copy still lands on the matched printing.
    expect(session.target.quantity).toBe(0)
    expect(session.target.foil_quantity).toBe(1)

    await session.confirmCurrent()
    expect(save).toHaveBeenCalledWith({
      game: 'mtg',
      id: target.id,
      quantity: 0,
      foil_quantity: 1,
    })
  })

  it('seeds the scanned copy as regular when no foil star was detected', async () => {
    await captureInto(false)
    expect(session.ready.value).toBe(true)
    expect(session.target.quantity).toBe(1)
    expect(session.target.foil_quantity).toBe(0)
  })

  it('adds the foil copy on top of an existing holding, never overwriting the base counts', async () => {
    // The scan write is absolute, so seeding a detected foil must preserve the copies already
    // owned in both finishes — a regression that dropped the base would delete real copies.
    ownedData.value = { quantity: 3, foil_quantity: 2 }
    const target = await captureInto(true)
    expect(session.target.quantity).toBe(3)
    expect(session.target.foil_quantity).toBe(3)

    await session.confirmCurrent()
    expect(save).toHaveBeenCalledWith({
      game: 'mtg',
      id: target.id,
      quantity: 3,
      foil_quantity: 3,
    })
  })

  it('upgrades a same-card rescan to foil when the star is detected only the second time', async () => {
    // First scan misses the star (e.g. OpenCV still loading), seeding a regular copy.
    await captureInto(false)
    expect(session.target.quantity).toBe(1)
    expect(session.target.foil_quantity).toBe(0)

    // Re-pointing the same card, now with the star detected, re-seeds onto foil.
    await session.handleCapture({
      fingerprints: [new Uint8Array(32)],
      setText: 'NEO • EN',
      foil: true,
    })
    await flushPromises()
    expect(session.target.quantity).toBe(0)
    expect(session.target.foil_quantity).toBe(1)
  })
})

describe('useScanSession repeat commits', () => {
  /** Mount a session holding one settled match: a single loaded printing, nothing owned yet. */
  async function captureOne() {
    const target = makeCard('neo-printing', { set_code: 'neo' })
    picker.printings.value = [target]
    picker.hasNextPage.value = false
    mocks.useScanMutation.mockReturnValue({
      mutateAsync: vi
        .fn<(...args: unknown[]) => Promise<{ data: Array<{ card: Card; distance: number }> }>>()
        .mockResolvedValue({ data: [{ card: target, distance: 0 }] }),
    })
    const Host = defineComponent({
      setup() {
        session = useScanSession(ref('mtg'))
        return () => null
      },
    })
    wrapper = mount(Host)
    await session.handleCapture({
      fingerprints: [new Uint8Array(32)],
      setText: 'NEO • EN',
      foil: false,
    })
    await flushPromises()
    return target
  }

  // A page-hide finalize and the auto-advance commit inside handleCapture both call
  // commitCurrent for the same on-screen match. They share `commitInFlight` only while one is
  // actually in flight; once it settles, the second call is a fresh commit of a panel whose
  // counts are already written. It must not write — or log — that card a second time.
  it('is a no-op once the match on screen has already been written', async () => {
    await captureOne()
    expect(session.ready.value).toBe(true)
    expect(session.target.quantity).toBe(1)

    expect(await session.commitCurrent()).toBe(true)
    expect(save).toHaveBeenCalledOnce()
    expect(session.log.value).toHaveLength(1)

    // Same untouched panel: the counts on screen are exactly what was written.
    expect(await session.commitCurrent()).toBe(false)
    expect(save).toHaveBeenCalledOnce()
    expect(session.log.value).toHaveLength(1)
  })

  it('still writes when the user adjusts the counts after a commit', async () => {
    const target = await captureOne()
    await session.commitCurrent()
    save.mockClear()

    session.adjust('quantity', 1)
    expect(await session.commitCurrent()).toBe(true)

    // The new entry's Undo must restore the previously committed count, not the count from
    // before the first commit — the baseline moved with the write.
    expect(save).toHaveBeenCalledExactlyOnceWith({
      game: 'mtg',
      id: target.id,
      quantity: 2,
      foil_quantity: 0,
    })
    expect(session.log.value[0]?.previous).toEqual({ quantity: 1, foil_quantity: 0 })
  })
})
