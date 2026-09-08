import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { ApiError } from '@/lib/api'
import type { CollectionAddSummary } from '@/lib/api'

import AddToCollectionButton from '../AddToCollectionButton.vue'

const PassThrough = defineComponent({ template: '<div><slot /></div>' })
const ButtonStub = defineComponent({
  inheritAttrs: false,
  template: '<button v-bind="$attrs"><slot /></button>',
})
// The dialog root stub renders its content whether or not it is "open", so the test reads the
// confirmation without driving reka-ui's portal. It still takes the `open` v-model, so a test
// can close and reopen the dialog by emitting `update:open` — which is what resets last
// time's result (see the reopen test).
const DialogRootStub = defineComponent({
  name: 'DialogRootStub',
  props: { open: Boolean },
  emits: ['update:open'],
  template: '<div><slot /></div>',
})
const DialogTriggerStub = defineComponent({
  name: 'DialogTriggerStub',
  template: '<div data-testid="trigger"><slot /></div>',
})

const submit = vi.fn<() => Promise<CollectionAddSummary>>()

function mountButton(copies = 100, note = 'Every card in this deck and its sideboard.') {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: PassThrough }],
  })
  return mount(AddToCollectionButton, {
    props: { copies, game: 'mtg', note, submit },
    global: {
      plugins: [router],
      stubs: {
        Button: ButtonStub,
        Dialog: DialogRootStub,
        DialogTrigger: DialogTriggerStub,
        DialogClose: ButtonStub,
        DialogContent: PassThrough,
        DialogDescription: PassThrough,
        DialogTitle: PassThrough,
        RouterLink: defineComponent({
          props: { to: String },
          template: '<a :href="to" v-bind="$attrs"><slot /></a>',
        }),
      },
    },
  })
}

/** The dialog's confirm button — NOT the trigger, which carries the same label. */
function confirmButton(wrapper: ReturnType<typeof mountButton>) {
  return wrapper.get('[data-testid="confirm-add"]')
}

describe('AddToCollectionButton', () => {
  beforeEach(() => {
    submit.mockReset()
  })

  it('asks before it writes, naming the copies and the additive rule', () => {
    const wrapper = mountButton(100, 'Every card in this deck and its sideboard.')

    expect(wrapper.get('[data-testid="trigger"]').text()).toContain('Add to collection')
    expect(wrapper.text()).toContain('Add 100 cards to your collection?')
    expect(wrapper.text()).toContain('Every card in this deck and its sideboard.')
    // The write is not idempotent, and the dialog is where the reader learns that.
    expect(wrapper.text()).toContain('adding the same deck twice records two copies')
    expect(submit).not.toHaveBeenCalled()
  })

  it('singularises a one-card deck', () => {
    const wrapper = mountButton(1)
    expect(wrapper.text()).toContain('Add 1 card to your collection?')
  })

  it('runs the write on confirm and reports what landed, with a way into the collection', async () => {
    submit.mockResolvedValueOnce({
      cards: 87,
      regular_copies: 99,
      foil_copies: 1,
      skipped_cards: 0,
    })
    const wrapper = mountButton()

    await confirmButton(wrapper).trigger('click')
    await flushPromises()

    expect(submit).toHaveBeenCalledOnce()
    expect(wrapper.text()).toContain('Added to your collection')
    expect(wrapper.text()).toContain('Added 100 cards (99 regular, 1 foil) across 87 printings.')
    expect(wrapper.text()).not.toContain("couldn't be added")
    expect(wrapper.get('[data-testid="view-collection"]').attributes('href')).toBe(
      '/collection/mtg',
    )
    // The confirmation is gone: nothing left to click that would add a second copy.
    expect(wrapper.find('[data-testid="confirm-add"]').exists()).toBe(false)
  })

  it('asks afresh when reopened after a success, so a second add is a second decision', async () => {
    submit.mockResolvedValueOnce({ cards: 2, regular_copies: 6, foil_copies: 0, skipped_cards: 0 })
    const wrapper = mountButton()

    await confirmButton(wrapper).trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('Added to your collection')

    // Close ("Done") and reopen (the trigger): the stub's v-model round-trips `open`.
    const dialog = wrapper.findComponent({ name: 'DialogRootStub' })
    dialog.vm.$emit('update:open', false)
    await flushPromises()
    dialog.vm.$emit('update:open', true)
    await flushPromises()

    expect(wrapper.text()).not.toContain('Added to your collection')
    expect(wrapper.text()).toContain('Add 100 cards to your collection?')
    expect(wrapper.find('[data-testid="confirm-add"]').exists()).toBe(true)
  })

  // The write is not idempotent, so this guard is the difference between one copy and two
  // for a user who double-clicks on a slow connection.
  it('sends one request when confirm is clicked again while the first is in flight', async () => {
    let finish!: (added: CollectionAddSummary) => void
    submit.mockReturnValueOnce(
      new Promise<CollectionAddSummary>((resolve) => {
        finish = resolve
      }),
    )
    const wrapper = mountButton()

    await confirmButton(wrapper).trigger('click')
    await confirmButton(wrapper).trigger('click')
    expect(submit).toHaveBeenCalledOnce()
    expect(confirmButton(wrapper).attributes('disabled')).toBeDefined()
    expect(confirmButton(wrapper).text()).toContain('Adding…')

    finish({ cards: 1, regular_copies: 1, foil_copies: 0, skipped_cards: 0 })
    await flushPromises()
    expect(wrapper.text()).toContain('Added to your collection')
  })

  it('leaves the foil clause out when nothing foil was added', async () => {
    submit.mockResolvedValueOnce({ cards: 2, regular_copies: 6, foil_copies: 0, skipped_cards: 0 })
    const wrapper = mountButton()

    await confirmButton(wrapper).trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Added 6 cards across 2 printings.')
    expect(wrapper.text()).not.toContain('regular')
  })

  it('says how many rows could not be added when a card has left the catalog', async () => {
    submit.mockResolvedValueOnce({ cards: 5, regular_copies: 9, foil_copies: 0, skipped_cards: 3 })
    const wrapper = mountButton()

    await confirmButton(wrapper).trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain(
      "3 cards couldn't be added because they are no longer in the catalog.",
    )
  })

  it('shows the API error and keeps the confirmation so the user can retry', async () => {
    submit.mockRejectedValueOnce(new ApiError('this deck has no cards to add', 422))
    const wrapper = mountButton()

    await confirmButton(wrapper).trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('this deck has no cards to add')
    expect(wrapper.text()).toContain('Add 100 cards to your collection?')
    expect(confirmButton(wrapper).attributes('disabled')).toBeUndefined()
  })

  it('words an unexpected failure generically', async () => {
    submit.mockRejectedValueOnce(new TypeError('network down'))
    const wrapper = mountButton()

    await confirmButton(wrapper).trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('The cards could not be added. Please try again.')
  })
})
