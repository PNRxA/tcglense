import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'

const deleteMutation = vi.hoisted(() => vi.fn<(variables: unknown) => Promise<unknown>>())

interface StubDeck {
  id: number
  name: string
  folder_id: number | null
}
interface StubFolder {
  id: number
  name: string
}

// Per-test query state. The mocked composables read it when the component's `setup` runs, so
// each test arranges it *before* mounting.
const state = vi.hoisted(() => ({
  decks: undefined as { data: StubDeck[] } | undefined,
  folders: [] as StubFolder[],
  isLoadingError: false,
  isRefetchError: false,
}))

function deck(id: number, name: string, folderId: number | null = null): StubDeck {
  return { id, name, folder_id: folderId }
}

vi.mock('@/composables/useCatalog', async () => {
  const { ref: vueRef } = await import('vue')
  return {
    useGamesQuery: () => ({ data: vueRef({ data: [{ id: 'mtg', name: 'Magic' }] }) }),
  }
})

vi.mock('@/composables/useDecks', async () => {
  const { ref: vueRef } = await import('vue')
  const mutation = (result: unknown = {}) => ({
    mutateAsync: vi.fn<() => Promise<unknown>>(async () => result),
    isPending: vueRef(false),
  })
  return {
    useDecksQuery: () => ({
      data: vueRef(state.decks),
      isPending: vueRef(false),
      isError: vueRef(state.isLoadingError || state.isRefetchError),
      isLoadingError: vueRef(state.isLoadingError),
      isRefetchError: vueRef(state.isRefetchError),
    }),
    useFoldersQuery: () => ({
      data: vueRef({ data: state.folders }),
      isPending: vueRef(false),
    }),
    useCreateDeckMutation: () => mutation(),
    useCreateFolderMutation: () => mutation({ id: 1 }),
    useDeleteDeckMutation: () => ({
      mutateAsync: deleteMutation,
      isPending: vueRef(false),
    }),
    useDeleteFolderMutation: () => mutation(),
    useMoveDeckToFolderMutation: () => mutation(),
  }
})

vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({ sessionResolved: true, isAuthenticated: true }),
}))

vi.mock('@/lib/seo', () => ({ usePageMeta: vi.fn<() => void>() }))

import GameDecksView from '../GameDecksView.vue'

const PassThrough = defineComponent({ template: '<div><slot /></div>' })
const ButtonStub = defineComponent({
  inheritAttrs: false,
  template: '<button v-bind="$attrs"><slot /></button>',
})
const DialogStub = defineComponent({
  props: { open: Boolean },
  emits: ['update:open'],
  template: '<div v-if="open"><slot /></div>',
})
const DeckTileStub = defineComponent({
  props: ['deck'],
  emits: ['move', 'remove'],
  template:
    '<button class="remove-deck" @click="$emit(\'remove\')">Remove {{ deck.name }}</button>',
})

function mountView() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/', component: PassThrough }],
  })
  return mount(GameDecksView, {
    props: { game: 'mtg' },
    global: {
      plugins: [router],
      stubs: {
        Button: ButtonStub,
        DeckImportDialog: PassThrough,
        DeckTile: DeckTileStub,
        Dialog: DialogStub,
        DialogClose: ButtonStub,
        DialogContent: PassThrough,
        DialogDescription: PassThrough,
        DialogTitle: PassThrough,
        DialogTrigger: ButtonStub,
        Select: PassThrough,
        SelectContent: PassThrough,
        SelectItem: PassThrough,
        SelectTrigger: PassThrough,
        SelectValue: PassThrough,
      },
    },
  })
}

beforeEach(() => {
  state.decks = { data: [deck(7, 'Test Deck')] }
  state.folders = []
  state.isLoadingError = false
  state.isRefetchError = false
})

describe('GameDecksView deck deletion', () => {
  beforeEach(() => {
    deleteMutation.mockReset()
    deleteMutation.mockResolvedValue({})
  })

  it('confirms through a shadcn dialog before deleting', async () => {
    const confirm = vi.fn<() => boolean>(() => true)
    vi.stubGlobal('confirm', confirm)
    const wrapper = mountView()

    await wrapper.find('.remove-deck').trigger('click')
    expect(confirm).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('Delete Test Deck?')
    expect(wrapper.text()).toContain('cannot be undone')

    const deleteButton = wrapper
      .findAll('button')
      .find((button) => button.text().trim() === 'Delete deck')
    if (!deleteButton) throw new Error('missing Delete deck button')
    await deleteButton.trigger('click')
    await flushPromises()

    expect(deleteMutation).toHaveBeenCalledExactlyOnceWith({ game: 'mtg', deckId: 7 })
    expect(wrapper.text()).not.toContain('cannot be undone')
    wrapper.unmount()
    vi.unstubAllGlobals()
  })
})

// Issue #622 (1): the deck list and the folder list are two independent queries, so a deck can
// carry a folder id the folders response no longer knows about — most visibly in the window
// between a folder deletion's two refetches, where the folders response lands first.
describe('GameDecksView folder grouping', () => {
  it('renders a deck whose folder is unknown as loose rather than dropping it', () => {
    state.folders = [{ id: 1, name: 'Standard' }]
    state.decks = {
      data: [deck(7, 'In A Live Folder', 1), deck(8, 'Orphaned', 99), deck(9, 'Loose')],
    }
    const wrapper = mountView()

    // Every deck the header counts is actually on the page — that's the invariant that broke.
    expect(wrapper.text()).toContain('3 deck(s)')
    const rendered = wrapper.findAll('.remove-deck').map((tile) => tile.text())
    expect(rendered).toHaveLength(3)
    expect(rendered.join(' ')).toContain('Orphaned')

    wrapper.unmount()
  })

  it('keeps every deck when the folders query returns nothing', () => {
    state.folders = []
    state.decks = { data: [deck(7, 'Filed Away', 1), deck(8, 'Loose')] }
    const wrapper = mountView()

    expect(wrapper.findAll('.remove-deck')).toHaveLength(2)
    wrapper.unmount()
  })

  it('files each deck under its own folder', () => {
    state.folders = [
      { id: 1, name: 'Standard' },
      { id: 2, name: 'Commander' },
    ]
    state.decks = { data: [deck(7, 'Mono Red', 1), deck(8, 'Atraxa', 2)] }
    const wrapper = mountView()

    const sections = wrapper.findAll('section').map((section) => section.text())
    expect(sections).toHaveLength(2)
    expect(sections[0]).toContain('Standard')
    expect(sections[0]).toContain('Mono Red')
    expect(sections[0]).not.toContain('Atraxa')
    expect(sections[1]).toContain('Commander')
    expect(sections[1]).toContain('Atraxa')

    wrapper.unmount()
  })
})

// Issue #622 (2): query-core keeps `data` when a refetch fails, so `isError` alone can't tell
// "nothing loaded" from "a background refresh hiccuped over a good list".
describe('GameDecksView load failures', () => {
  it('keeps the cached list and shows a quiet cue when a background refetch fails', () => {
    state.decks = { data: [deck(7, 'Test Deck')] }
    state.isRefetchError = true
    const wrapper = mountView()

    expect(wrapper.text()).not.toContain('Please retry')
    expect(wrapper.findAll('.remove-deck')).toHaveLength(1)
    expect(wrapper.text()).toContain("Couldn't refresh")

    wrapper.unmount()
  })

  it('shows the error state when the list never loaded', () => {
    state.decks = undefined
    state.isLoadingError = true
    const wrapper = mountView()

    expect(wrapper.text()).toContain("Couldn't load your decks. Please retry.")
    expect(wrapper.findAll('.remove-deck')).toHaveLength(0)

    wrapper.unmount()
  })

  it('shows no failure cue while the list is healthy', () => {
    const wrapper = mountView()

    expect(wrapper.text()).not.toContain("Couldn't refresh")
    expect(wrapper.text()).not.toContain('Please retry')

    wrapper.unmount()
  })
})
