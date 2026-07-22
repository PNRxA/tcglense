import { computed, defineComponent, h, inject, provide, type ComputedRef } from 'vue'
import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import DeckFormatField from '../DeckFormatField.vue'

const selectedValueKey = Symbol('selected-value')

const SelectStub = defineComponent({
  props: { modelValue: { type: String, required: true } },
  setup(props, { slots }) {
    provide(
      selectedValueKey,
      computed(() => props.modelValue),
    )
    return () => h('div', { 'data-test': 'select' }, slots.default?.())
  },
})

const SelectTriggerStub = defineComponent({
  inheritAttrs: false,
  setup(_, { attrs, slots }) {
    return () => h('button', attrs, slots.default?.())
  },
})

const SelectValueStub = defineComponent({
  props: { placeholder: { type: String, default: '' } },
  setup(props) {
    const selected = inject<ComputedRef<string>>(selectedValueKey)
    return () => {
      const value = selected?.value
      const text = value === 'none' ? 'No format' : value === 'custom' ? 'Custom…' : value
      return h('span', { 'data-test': 'select-value' }, text || props.placeholder)
    }
  },
})

const SelectContentStub = defineComponent({
  setup() {
    return () => h('div', { 'data-test': 'select-content' })
  },
})

function mountField(modelValue: string, game = 'mtg') {
  return mount(DeckFormatField, {
    props: { game, modelValue },
    global: {
      stubs: {
        Select: SelectStub,
        SelectTrigger: SelectTriggerStub,
        SelectValue: SelectValueStub,
        SelectContent: SelectContentStub,
      },
    },
  })
}

function selectedText(wrapper: ReturnType<typeof mountField>) {
  return wrapper.get('[data-test="select-value"]').text()
}

describe('DeckFormatField', () => {
  it('shows No format for an empty model without a text input', () => {
    const wrapper = mountField('')

    expect(selectedText(wrapper)).toBe('No format')
    expect(wrapper.find('input').exists()).toBe(false)
  })

  it('preselects the matching Commander option', () => {
    const wrapper = mountField('Commander')

    expect(selectedText(wrapper)).toBe('Commander')
    expect(wrapper.find('input').exists()).toBe(false)
  })

  it('resolves an EDH alias to Commander', () => {
    const wrapper = mountField('EDH')

    expect(selectedText(wrapper)).toBe('Commander')
    expect(wrapper.find('input').exists()).toBe(false)
  })

  it('shows a custom value in the free-text input', () => {
    const wrapper = mountField('my kitchen league')

    expect(selectedText(wrapper)).toBe('Custom…')
    expect(wrapper.get('input').element.value).toBe('my kitchen league')
  })

  it('emits model updates typed into the custom input', async () => {
    const wrapper = mountField('my kitchen league')

    await wrapper.get('input').setValue('our kitchen league')

    expect(wrapper.emitted('update:modelValue')?.slice(-1)[0]).toEqual(['our kitchen league'])
  })

  it('renders only a plain input for a game without curated formats', () => {
    const wrapper = mountField('homebrew', 'somegame')

    expect(wrapper.find('[data-test="select"]').exists()).toBe(false)
    expect(wrapper.get('input').attributes('placeholder')).toBe('Format (optional)')
    expect(wrapper.get('input').element.value).toBe('homebrew')
  })
})
