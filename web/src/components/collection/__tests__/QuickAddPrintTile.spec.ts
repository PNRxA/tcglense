import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { mount } from '@vue/test-utils'
import { makeCard } from '@/test/fixtures'

const adjust = vi.hoisted(() => vi.fn<(which: 'quantity' | 'foil', delta: number) => void>())

vi.mock('@/composables/useOwnedCountEditor', async () => {
  const { ref } = await import('vue')
  return {
    useOwnedCountEditor: () => ({
      adjust,
      regular: ref(2),
      foil: ref(1),
      saving: ref(false),
      saveError: ref(false),
    }),
  }
})

import QuickAddPrintTile from '@/components/collection/QuickAddPrintTile.vue'

const PrintingTileStub = defineComponent({
  template: '<div><slot name="actions" /></div>',
})
const ButtonStub = defineComponent({
  inheritAttrs: false,
  template: '<button v-bind="$attrs"><slot /></button>',
})

function mountTile(ready: boolean) {
  return mount(QuickAddPrintTile, {
    props: {
      game: 'mtg',
      card: makeCard('island'),
      seed: ready ? { quantity: 2, foil_quantity: 1 } : undefined,
      ready,
    },
    global: { stubs: { Button: ButtonStub, PrintingTile: PrintingTileStub } },
  })
}

beforeEach(() => adjust.mockReset())

describe('QuickAddPrintTile action adapter', () => {
  it('gates absolute-count writes on the authoritative seed', async () => {
    const wrapper = mountTile(false)
    expect(
      wrapper.findAll('button').every((button) => button.attributes('disabled') !== undefined),
    ).toBe(true)

    await wrapper.setProps({ ready: true, seed: { quantity: 2, foil_quantity: 1 } })
    await wrapper.get('button[aria-label^="Add one regular"]').trigger('click')
    await wrapper.get('button[aria-label^="Add one foil"]').trigger('click')

    expect(adjust).toHaveBeenNthCalledWith(1, 'quantity', 1)
    expect(adjust).toHaveBeenNthCalledWith(2, 'foil', 1)
  })
})
