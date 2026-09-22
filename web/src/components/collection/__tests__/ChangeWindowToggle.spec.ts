import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import ChangeWindowToggle from '../ChangeWindowToggle.vue'
import type { MoverWindow } from '@/lib/api'

describe('ChangeWindowToggle', () => {
  it('renders the seven windows in order, pressing the active one', () => {
    const wrapper = mount(ChangeWindowToggle, {
      props: { modelValue: 'week' as MoverWindow, label: 'Collection value change window' },
    })
    const buttons = wrapper.findAll('button')
    expect(buttons.map((b) => b.text())).toEqual(['1D', '7D', '30D', '1Y', '2Y', '3Y', 'All'])
    expect(buttons.map((b) => b.attributes('aria-pressed'))).toEqual([
      'false',
      'true',
      'false',
      'false',
      'false',
      'false',
      'false',
    ])
    expect(wrapper.find('[role=group]').attributes('aria-label')).toBe(
      'Collection value change window',
    )
  })

  it('emits the picked window token through v-model', async () => {
    const wrapper = mount(ChangeWindowToggle, {
      props: {
        modelValue: 'week' as MoverWindow,
        label: 'w',
        'onUpdate:modelValue': (value: MoverWindow) => wrapper.setProps({ modelValue: value }),
      },
    })
    await wrapper
      .findAll('button')
      .find((b) => b.text() === 'All')!
      .trigger('click')
    expect(wrapper.emitted('update:modelValue')).toEqual([['all_time']])
    expect(wrapper.findAll('button')[6]!.attributes('aria-pressed')).toBe('true')
  })
})
