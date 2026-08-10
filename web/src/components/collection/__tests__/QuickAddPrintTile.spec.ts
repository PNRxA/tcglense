import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { mount } from '@vue/test-utils'
import { makeCard } from '@/test/fixtures'

const H = vi.hoisted(() => ({
  adjust: vi.fn<(which: 'quantity' | 'foil', delta: number) => void>(),
  // The editor's landed-write callback, captured so a test can fire one without a network.
  onSaved: undefined as ((write: unknown) => void) | undefined,
}))
const adjust = H.adjust

vi.mock('@/composables/useOwnedCountEditor', async () => {
  const { ref } = await import('vue')
  return {
    useOwnedCountEditor: (
      _game: unknown,
      _cardId: unknown,
      _seed: unknown,
      opts?: { onSaved?: (write: unknown) => void },
    ) => {
      H.onSaved = opts?.onSaved
      return {
        adjust,
        regular: ref(2),
        foil: ref(1),
        saving: ref(false),
        saveError: ref(false),
      }
    },
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

  it('reports a landed write upward with the printing it was for', () => {
    // The tile is the only place that knows *which card* the editor's id belongs to, so it
    // is what turns a write into something a host page can log (the scan page files it in
    // the session history, undo and all).
    const card = makeCard('island')
    const reportSaved = vi.fn<(write: unknown) => void>()
    mount(QuickAddPrintTile, {
      props: {
        game: 'mtg',
        card,
        seed: { quantity: 2, foil_quantity: 1 },
        ready: true,
        reportSaved,
      },
      global: { stubs: { Button: ButtonStub, PrintingTile: PrintingTileStub } },
    })

    H.onSaved!({
      id: 'island',
      quantity: 3,
      foil_quantity: 1,
      previous: { quantity: 2, foil_quantity: 1 },
    })

    expect(reportSaved).toHaveBeenCalledExactlyOnceWith({
      id: 'island',
      quantity: 3,
      foil_quantity: 1,
      previous: { quantity: 2, foil_quantity: 1 },
      card,
    })
  })

  it('still reports a write that lands after the tile is gone', () => {
    // The save is debounced and the editor deliberately flushes on unmount, so "tap +, tap
    // Done" resolves the write after this tile (and the dialog around it) have unmounted.
    // Vue drops an `emit` from an unmounted instance, which would land the copy in the
    // collection with no history row, no undo, and no rebase of an open tentative match —
    // so the report is a callback prop, which has no such guard.
    const card = makeCard('island')
    const reportSaved = vi.fn<(write: unknown) => void>()
    const wrapper = mount(QuickAddPrintTile, {
      props: {
        game: 'mtg',
        card,
        seed: { quantity: 0, foil_quantity: 0 },
        ready: true,
        reportSaved,
      },
      global: { stubs: { Button: ButtonStub, PrintingTile: PrintingTileStub } },
    })
    const landed = H.onSaved!

    wrapper.unmount()
    landed({
      id: 'island',
      quantity: 1,
      foil_quantity: 0,
      previous: { quantity: 0, foil_quantity: 0 },
    })

    expect(reportSaved).toHaveBeenCalledOnce()
  })
})
