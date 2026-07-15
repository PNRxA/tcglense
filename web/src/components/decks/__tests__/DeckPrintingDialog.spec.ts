import { describe, expect, it, vi } from 'vitest'
import { defineComponent, nextTick, ref } from 'vue'
import { mount } from '@vue/test-utils'
import type { Card } from '@/lib/api'

const queryState = vi.hoisted(() => ({ page: undefined as { value: number } | undefined }))

vi.mock('@/composables/useQuickAdd', async () => {
  const { computed, ref: vueRef } = await import('vue')
  return {
    useCardPrintingsByName: (
      _game: unknown,
      _name: unknown,
      opts: { page?: { value: number } },
    ) => {
      queryState.page = opts.page
      return {
        data: computed(() => ({
          data: [
            {
              id: `print-${opts.page?.value ?? 1}`,
              name: 'Island',
              set_name: `Set page ${opts.page?.value ?? 1}`,
              set_code: 'tst',
              collector_number: String(opts.page?.value ?? 1),
              has_image: false,
            },
          ],
          page: opts.page?.value ?? 1,
          page_size: 200,
          total: 816,
          has_more: (opts.page?.value ?? 1) < 5,
        })),
        isPending: vueRef(false),
        isFetching: vueRef(false),
        isError: vueRef(false),
      }
    },
  }
})

vi.mock('@/composables/useDecks', () => ({
  useChangeDeckCardPrintingMutation: () => ({
    mutateAsync: vi.fn<(variables: unknown) => Promise<void>>(),
    isPending: ref(false),
  }),
}))

import DeckPrintingDialog from '../DeckPrintingDialog.vue'

const PassThrough = defineComponent({ template: '<div><slot /></div>' })
const ButtonStub = defineComponent({
  inheritAttrs: false,
  template: '<button v-bind="$attrs"><slot /></button>',
})

describe('DeckPrintingDialog pagination', () => {
  it('navigates beyond the first 200 exact-name printings', async () => {
    const wrapper = mount(DeckPrintingDialog, {
      props: {
        open: true,
        game: 'mtg',
        deckId: 1,
        sectionId: 2,
        card: { id: 'current', name: 'Island' } as Card,
        quantity: 4,
        foilQuantity: 0,
      },
      global: {
        stubs: {
          Button: ButtonStub,
          CardImage: PassThrough,
          Dialog: PassThrough,
          DialogClose: ButtonStub,
          DialogContent: PassThrough,
          DialogDescription: PassThrough,
          DialogTitle: PassThrough,
        },
      },
    })

    expect(wrapper.text()).toContain('Showing 1–200 of 816 printings')
    expect(wrapper.text()).toContain('1 / 5')
    expect(queryState.page?.value).toBe(1)

    const next = wrapper.findAll('button').find((button) => button.text().trim() === 'Next')
    if (!next) throw new Error('missing Next button')
    await next.trigger('click')
    await nextTick()

    expect(queryState.page?.value).toBe(2)
    expect(wrapper.text()).toContain('Showing 201–400 of 816 printings')
    expect(wrapper.text()).toContain('Set page 2')
    expect(wrapper.text()).toContain('2 / 5')
  })
})
