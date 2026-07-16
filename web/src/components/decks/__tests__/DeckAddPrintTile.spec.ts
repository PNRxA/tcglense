import { describe, expect, it } from 'vitest'
import { defineComponent } from 'vue'
import { mount } from '@vue/test-utils'
import DeckAddPrintTile from '@/components/decks/DeckAddPrintTile.vue'
import PrintingTile from '@/components/printings/PrintingTile.vue'
import { makeCard } from '@/test/fixtures'

const PrintingTileStub = defineComponent({
  props: {
    disabled: Boolean,
    loading: Boolean,
    ariaLabel: String,
  },
  emits: ['select'],
  template: `
    <button :disabled="disabled" :aria-label="ariaLabel" @click="$emit('select')">
      <slot name="overlay" />
    </button>
  `,
})

function mountTile(overrides: { disabled?: boolean; loading?: boolean } = {}) {
  return mount(DeckAddPrintTile, {
    props: { game: 'mtg', card: makeCard('island'), count: 3, ...overrides },
    global: { stubs: { PrintingTile: PrintingTileStub } },
  })
}

describe('DeckAddPrintTile action adapter', () => {
  it('keeps the target count and supports rapid additive clicks', async () => {
    const wrapper = mountTile()
    expect(wrapper.text()).toContain('×3')

    await wrapper.get('button').trigger('click')
    await wrapper.get('button').trigger('click')
    expect(wrapper.emitted('add')).toHaveLength(2)
  })

  it('forwards loading and safe unclassified-card disabling', async () => {
    const wrapper = mountTile({ disabled: true, loading: true })
    expect(wrapper.getComponent(PrintingTile).props('loading')).toBe(true)
    expect(wrapper.get('button').attributes('disabled')).toBeDefined()

    await wrapper.get('button').trigger('click')
    expect(wrapper.emitted('add')).toBeUndefined()
  })
})
