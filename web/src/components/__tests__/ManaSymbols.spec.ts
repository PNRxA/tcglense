import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import ManaSymbols from '../cards/ManaSymbols.vue'

describe('ManaSymbols', () => {
  it('renders each symbol as a mana-font icon with an accessible label', () => {
    const wrapper = mount(ManaSymbols, { props: { text: '{2}{W}' } })
    expect(wrapper.findAll('i')).toHaveLength(2)
    const generic = wrapper.get('.ms-2')
    const white = wrapper.get('.ms-w')
    expect(generic.classes()).toEqual(expect.arrayContaining(['ms', 'ms-cost']))
    expect(white.classes()).toEqual(expect.arrayContaining(['ms', 'ms-cost']))
    expect(white.attributes('role')).toBe('img')
    expect(white.attributes('aria-label')).toBe('White mana')
    // A pure mana cost contributes no text between the pips.
    expect(wrapper.element.textContent).toBe('')
  })

  it('keeps surrounding words as text without injecting stray whitespace', () => {
    const wrapper = mount(ManaSymbols, { props: { text: '{T}: Add {G}.' } })
    expect(wrapper.findAll('i')).toHaveLength(2)
    // The tap pip sits flush against the ":" — no space introduced by the template.
    expect(wrapper.element.textContent).toBe(': Add .')
  })

  it('leaves an unrecognised token as literal text', () => {
    const wrapper = mount(ManaSymbols, { props: { text: 'Add {FOO}' } })
    expect(wrapper.findAll('i')).toHaveLength(0)
    expect(wrapper.element.textContent).toBe('Add {FOO}')
  })
})
