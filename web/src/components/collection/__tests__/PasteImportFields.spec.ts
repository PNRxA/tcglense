import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import PasteImportFields from '@/components/collection/PasteImportFields.vue'

// The paste tab (issue #572). Two things matter here: the box is a real two-way binding
// (the parent submits what it holds), and the line counter reflects what will actually be
// read — blank lines aren't rows, so counting them would over-promise.

describe('PasteImportFields', () => {
  it('binds the textarea to the model in both directions', async () => {
    const wrapper = mount(PasteImportFields, {
      props: { modelValue: '2 Sol Ring (C21) 263' },
    })
    const textarea = wrapper.get('textarea')
    expect(textarea.element.value).toBe('2 Sol Ring (C21) 263')

    await textarea.setValue('4 Counterspell')
    const emitted = wrapper.emitted('update:modelValue') ?? []
    expect(emitted[emitted.length - 1]).toEqual(['4 Counterspell'])
  })

  it('counts only non-blank lines, and says nothing when empty', () => {
    const empty = mount(PasteImportFields, { props: { modelValue: '   \n\n' } })
    expect(empty.text()).not.toContain('pasted')

    const one = mount(PasteImportFields, { props: { modelValue: '2 Sol Ring (C21) 263' } })
    expect(one.text()).toContain('1 line pasted')

    const many = mount(PasteImportFields, {
      props: { modelValue: '2 Sol Ring (C21) 263\n\n4 Counterspell\n   \n1 Black Lotus\n' },
    })
    expect(many.text()).toContain('3 lines pasted')
  })

  it('tells the user how to export from Mythic Tools', () => {
    const wrapper = mount(PasteImportFields, { props: { modelValue: '' } })
    expect(wrapper.text()).toContain('Mythic Tools')
  })
})
