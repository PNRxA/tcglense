import { describe, expect, it } from 'vitest'
import { defineComponent, ref } from 'vue'
import { mount } from '@vue/test-utils'
import type { DeckFilterColor } from '@/lib/deckFilter'
import DeckColorFilter from '../DeckColorFilter.vue'

function mountFilter(initial: DeckFilterColor[] = []) {
  const selected = ref<DeckFilterColor[]>(initial)
  const wrapper = mount(
    defineComponent({
      components: { DeckColorFilter },
      setup: () => ({ selected }),
      template: '<DeckColorFilter v-model="selected" />',
    }),
  )
  return { wrapper, selected }
}

describe('DeckColorFilter', () => {
  it('renders one pressable pip per colour plus colourless', () => {
    const { wrapper } = mountFilter(['U'])
    const buttons = wrapper.findAll('button')
    expect(buttons).toHaveLength(6)
    expect(buttons.map((b) => b.attributes('aria-pressed'))).toEqual([
      'false',
      'true',
      'false',
      'false',
      'false',
      'false',
    ])
  })

  it('toggles a colour in and out of the selection', async () => {
    const { wrapper, selected } = mountFilter()
    const white = wrapper.get('button[aria-label="Filter to white"]')
    await white.trigger('click')
    expect(selected.value).toEqual(['W'])
    await wrapper.get('button[aria-label="Filter to colorless"]').trigger('click')
    expect(selected.value).toEqual(['W', 'C'])
    await white.trigger('click')
    expect(selected.value).toEqual(['C'])
  })
})
