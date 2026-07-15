import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'

const deleteMutation = vi.hoisted(() => vi.fn<(variables: unknown) => Promise<unknown>>())

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
      data: vueRef({
        data: [
          {
            id: 7,
            game: 'mtg',
            name: 'Test Deck',
            description: null,
            format: null,
            folder_id: null,
            is_public: false,
            card_count: 60,
            created_at: '',
            updated_at: '',
          },
        ],
      }),
      isPending: vueRef(false),
      isError: vueRef(false),
    }),
    useFoldersQuery: () => ({
      data: vueRef({ data: [] }),
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

describe('GameDecksView deck deletion', () => {
  beforeEach(() => {
    deleteMutation.mockReset()
    deleteMutation.mockResolvedValue({})
  })

  it('confirms through a shadcn dialog before deleting', async () => {
    const confirm = vi.fn<() => boolean>(() => true)
    vi.stubGlobal('confirm', confirm)
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [{ path: '/', component: PassThrough }],
    })
    const wrapper = mount(GameDecksView, {
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
