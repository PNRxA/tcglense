import { describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { mount } from '@vue/test-utils'
import PrintingTile from '@/components/printings/PrintingTile.vue'
import { makeCard } from '@/test/fixtures'

vi.mock('@/composables/useCurrency', () => ({
  useCurrency: () => ({ formatUsd: (amount: string) => `A$${amount}` }),
}))

const CardImageStub = defineComponent({
  props: { id: String },
  template: '<div :data-image-id="id" />',
})

describe('PrintingTile', () => {
  it('shows the shared artwork, metadata, display price, and current state', () => {
    const wrapper = mount(PrintingTile, {
      props: {
        game: 'mtg',
        card: makeCard('island', {
          set_name: 'Alpha Set',
          set_code: 'alp',
          collector_number: '42',
          rarity: 'rare',
          prices: { usd: '2.50', usd_foil: null, eur: null, tix: null },
        }),
        selectable: true,
        current: true,
        disabled: true,
      },
      global: { stubs: { CardImage: CardImageStub } },
    })

    expect(wrapper.find('[data-image-id="island"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('Alpha Set')
    expect(wrapper.text()).toContain('ALP · #42 · rare')
    expect(wrapper.text()).toContain('A$2.50')
    expect(wrapper.text()).toContain('Current')
    expect(wrapper.get('button').attributes('disabled')).toBeDefined()
  })

  it('exposes the common loading state', () => {
    const wrapper = mount(PrintingTile, {
      props: { game: 'mtg', card: makeCard('island'), selectable: true, loading: true },
      global: { stubs: { CardImage: CardImageStub } },
    })
    expect(wrapper.get('button').attributes('aria-busy')).toBe('true')
  })
})
