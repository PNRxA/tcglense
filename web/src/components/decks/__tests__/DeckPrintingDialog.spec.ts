import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import type { Card } from '@/lib/api'
import { makeCard } from '@/test/fixtures'

const mocks = vi.hoisted(() => ({
  mutateAsync: vi.fn<(variables: unknown) => Promise<void>>(),
  loadMore: vi.fn<() => Promise<void>>(),
}))

vi.mock('@/composables/usePrintings', async () => {
  const { computed, ref } = await import('vue')
  const printings = ref([
    {
      id: 'current',
      name: 'Island',
      set_name: 'Current Set',
      set_code: 'cur',
      collector_number: '1',
    },
    {
      id: 'target',
      name: 'Island',
      set_name: 'Target Set',
      set_code: 'tgt',
      collector_number: '201',
    },
  ] as Card[])
  const filter = ref('')
  const collectionOnly = ref(false)
  return {
    usePrintingPicker: () => ({
      filter,
      collectionOnly,
      collectionFilterLoading: ref(false),
      printings,
      filteredPrintings: computed(() => printings.value),
      total: ref(816),
      isPending: ref(false),
      failed: ref(false),
      hasNextPage: ref(true),
      isFetchingNextPage: ref(false),
      loadMore: mocks.loadMore,
    }),
  }
})

vi.mock('@/composables/useDecks', async () => {
  const { ref } = await import('vue')
  return {
    useChangeDeckCardPrintingMutation: () => ({
      mutateAsync: mocks.mutateAsync,
      isPending: ref(false),
    }),
  }
})

import DeckPrintingDialog from '@/components/decks/DeckPrintingDialog.vue'

const PassThrough = defineComponent({ template: '<div><slot /></div>' })
const ButtonStub = defineComponent({
  inheritAttrs: false,
  template: '<button v-bind="$attrs"><slot /></button>',
})
const PrintingTileStub = defineComponent({
  props: {
    card: { type: Object, required: true },
    current: Boolean,
    disabled: Boolean,
  },
  emits: ['select'],
  template: `
    <button :data-id="card.id" :disabled="disabled" @click="$emit('select')">
      {{ card.set_name }}<span v-if="current"> Current</span>
    </button>
  `,
})

function mountDialog() {
  return mount(DeckPrintingDialog, {
    props: {
      open: true,
      game: 'mtg',
      deckId: 1,
      sectionId: 2,
      card: makeCard('current'),
      quantity: 3,
      foilQuantity: 1,
    },
    global: {
      stubs: {
        Button: ButtonStub,
        Dialog: PassThrough,
        DialogClose: ButtonStub,
        DialogContent: PassThrough,
        DialogDescription: PassThrough,
        DialogTitle: PassThrough,
        PrintingTile: PrintingTileStub,
      },
    },
  })
}

beforeEach(() => {
  mocks.mutateAsync.mockReset().mockResolvedValue(undefined)
  mocks.loadMore.mockReset().mockResolvedValue(undefined)
})

describe('DeckPrintingDialog action adapter', () => {
  it('uses the shared loaded-page scope and pagination control', async () => {
    const wrapper = mountDialog()
    expect(wrapper.text()).toContain('2 of 816 printings loaded')
    expect(wrapper.text()).toContain('Filter searches loaded printings only.')
    expect(wrapper.get('[data-id="current"]').text()).toContain('Current')

    const loadMore = wrapper
      .findAll('button')
      .find((button) => button.text().includes('Load more printings'))
    if (!loadMore) throw new Error('missing load-more button')
    await loadMore.trigger('click')
    expect(mocks.loadMore).toHaveBeenCalledOnce()
  })

  it('offers the collection filter and reflects the picker toggle', async () => {
    const wrapper = mountDialog()
    const checkbox = wrapper.get('input[type="checkbox"]')
    expect(wrapper.text()).toContain('In my collection')
    expect((checkbox.element as HTMLInputElement).checked).toBe(false)

    // Ticking it flips the shared picker's `collectionOnly` (v-model), which repaints the box.
    await checkbox.setValue(true)
    expect((checkbox.element as HTMLInputElement).checked).toBe(true)
  })

  it('performs one atomic replacement and closes on success', async () => {
    const wrapper = mountDialog()
    await wrapper.get('[data-id="target"]').trigger('click')
    await flushPromises()

    expect(mocks.mutateAsync).toHaveBeenCalledWith({
      game: 'mtg',
      deckId: 1,
      sectionId: 2,
      id: 'current',
      newCardId: 'target',
    })
    const openEvents = wrapper.emitted('update:open') ?? []
    expect(openEvents[openEvents.length - 1]).toEqual([false])
  })
})
