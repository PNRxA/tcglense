import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, nextTick, ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'

const mutation = vi.hoisted(() => ({
  mutateAsync: vi.fn<(variables: unknown) => Promise<unknown>>(),
  reset: vi.fn<() => void>(),
}))

vi.mock('@/composables/useDecks', () => ({
  useImportDeckMutation: () => ({
    mutateAsync: mutation.mutateAsync,
    reset: mutation.reset,
    isPending: ref(false),
  }),
}))

import DeckImportDialog from '../DeckImportDialog.vue'

const PassThrough = defineComponent({ template: '<div><slot /></div>' })
const ButtonStub = defineComponent({
  inheritAttrs: false,
  template: '<button v-bind="$attrs"><slot /></button>',
})
const DialogRootStub = defineComponent({
  name: 'DialogRootStub',
  props: { open: Boolean },
  emits: ['update:open'],
  template: '<div><slot /></div>',
})
const SelectStub = defineComponent({
  name: 'SelectRootStub',
  props: { modelValue: String },
  emits: ['update:modelValue'],
  template: '<div data-testid="provider-select"><slot /></div>',
})
const SelectItemStub = defineComponent({
  name: 'SelectItem',
  props: { value: String, disabled: Boolean },
  template:
    '<div data-testid="provider-option" :data-value="value" :data-disabled="disabled"><slot /></div>',
})

function mountDialog() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/', component: PassThrough }],
  })
  return mount(DeckImportDialog, {
    props: { game: 'mtg' },
    global: {
      plugins: [router],
      stubs: {
        Button: ButtonStub,
        Dialog: DialogRootStub,
        DialogTrigger: ButtonStub,
        DialogClose: ButtonStub,
        DialogContent: PassThrough,
        DialogDescription: PassThrough,
        DialogTitle: PassThrough,
        Select: SelectStub,
        SelectTrigger: PassThrough,
        SelectContent: PassThrough,
        SelectItem: SelectItemStub,
        SelectValue: PassThrough,
      },
    },
  })
}

function buttonNamed(wrapper: ReturnType<typeof mountDialog>, name: string) {
  const button = wrapper.findAll('button').find((candidate) => candidate.text().trim() === name)
  if (!button) throw new Error(`missing ${name} button`)
  return button
}

describe('DeckImportDialog', () => {
  beforeEach(() => {
    mutation.mutateAsync.mockReset()
    mutation.reset.mockReset()
  })

  it('uses the shadcn select and switches provider-specific upload guidance', async () => {
    const wrapper = mountDialog()
    expect(wrapper.find('select').exists()).toBe(false)
    expect(wrapper.findComponent(SelectStub).exists()).toBe(true)

    const options = wrapper.findAllComponents(SelectItemStub)
    expect(options.find((option) => option.props('value') === 'moxfield')?.props('disabled')).toBe(
      true,
    )

    await buttonNamed(wrapper, 'Upload a file').trigger('click')
    expect(wrapper.text()).toContain(
      'Keep the CSV header row and include the Quantity, Name, Scryfall ID, and Categories columns.',
    )
    wrapper.findComponent(SelectStub).vm.$emit('update:modelValue', 'moxfield')
    await nextTick()
    expect(wrapper.text()).toContain('Upload a Moxfield CSV or plain-text deck export.')
    expect(wrapper.find('input[type="file"]').attributes('accept')).toContain('.txt')
    expect(
      wrapper
        .findAllComponents(SelectItemStub)
        .find((option) => option.props('value') === 'moxfield')
        ?.props('disabled'),
    ).toBe(false)

    await buttonNamed(wrapper, 'Paste a link').trigger('click')
    await nextTick()
    expect(wrapper.findComponent(SelectStub).props('modelValue')).toBe('archidekt')
    wrapper.unmount()
  })

  it('keeps the source controls keyboard-addressable tabs', async () => {
    const wrapper = mountDialog()
    const tabs = wrapper.findAll('[role="tab"]')
    expect(tabs).toHaveLength(2)
    expect(tabs.every((tab) => tab.element.tagName === 'BUTTON')).toBe(true)
    expect(tabs.every((tab) => tab.attributes('type') === 'button')).toBe(true)
    expect(tabs[0]?.attributes('aria-selected')).toBe('true')
    expect(tabs[1]?.attributes('aria-selected')).toBe('false')

    await tabs[1]!.trigger('click')
    expect(tabs[0]?.attributes('aria-selected')).toBe('false')
    expect(tabs[1]?.attributes('aria-selected')).toBe('true')
    wrapper.unmount()
  })

  it('surfaces an upload failure without navigating or reporting success', async () => {
    mutation.mutateAsync.mockRejectedValueOnce(new Error('failed'))
    const wrapper = mountDialog()
    await buttonNamed(wrapper, 'Upload a file').trigger('click')
    await wrapper.find('#deck-import-name').setValue('Broken import')
    const input = wrapper.find('input[type="file"]')
    Object.defineProperty(input.element, 'files', {
      configurable: true,
      value: [
        new File(['Quantity,Name,Scryfall ID,Categories\n1,Card,card-id,Mainboard\n'], 'deck.csv', {
          type: 'text/csv',
        }),
      ],
    })
    await input.trigger('change')
    await buttonNamed(wrapper, 'Import').trigger('click')
    await flushPromises()

    expect(mutation.mutateAsync).toHaveBeenCalledOnce()
    expect(wrapper.text()).toContain('The deck could not be imported. Please retry.')
    expect(wrapper.text()).not.toContain('was created.')
    wrapper.unmount()
  })
})
