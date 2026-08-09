import { describe, it, expect } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'

// A dialog panel's vertical placement is load-bearing, not cosmetic. Centred, the panel is
// pinned to the viewport's midpoint, so ANY height change moves everything already on
// screen by half the delta — and the detail modal's body fills in over several independent
// fetches (art tags, price chart, other printings, sealed products, rulings). That slid the
// collection steppers up between a user aiming at "Regular +" and the tap landing, which
// then hit "Foil +", the row directly below. `anchor="top"` pins the top edge so late
// content only extends downward. These tests pin both placements — and that a caller's own
// sizing classes still come through `cn` — so neither can be undone by accident.

/** Mount a dialog and read the portalled panel's class list off the document. */
async function contentClasses(anchor?: 'center' | 'top', extra?: string) {
  const wrapper = mount(
    {
      components: { Dialog, DialogContent, DialogTitle },
      props: {
        anchor: { type: String, default: undefined },
        extra: { type: String, default: undefined },
      },
      template: `<Dialog :open="true"><DialogContent :anchor="anchor" :class="extra"><DialogTitle>t</DialogTitle></DialogContent></Dialog>`,
    },
    { props: { anchor, extra }, attachTo: document.body },
  )
  await flushPromises()
  const classes = document.querySelector('[data-slot="dialog-content"]')?.className ?? ''
  wrapper.unmount()
  return classes
}

describe('DialogContent placement', () => {
  it('centres the panel by default', async () => {
    const classes = await contentClasses()
    expect(classes).toContain('top-1/2')
    expect(classes).toContain('-translate-y-1/2')
  })

  it('pins the top edge (and drops the centring transform) with anchor="top"', async () => {
    const classes = await contentClasses('top')
    // The panel hangs from a fixed top inset, so growth extends downward only...
    expect(classes).toContain('top-[max(0.75rem,5svh)]')
    // ...and neither half of the centring survives — either one left behind would keep the
    // panel drifting as its content lands.
    expect(classes).not.toContain('top-1/2')
    expect(classes).not.toContain('-translate-y-1/2')
  })

  it('keeps the horizontal centring and the caller-supplied classes either way', async () => {
    for (const anchor of [undefined, 'top'] as const) {
      const classes = await contentClasses(
        anchor,
        'max-h-[90svh] w-[min(96vw,64rem)] overflow-y-auto',
      )
      expect(classes).toContain('left-1/2')
      expect(classes).toContain('-translate-x-1/2')
      expect(classes).toContain('max-h-[90svh]')
      expect(classes).toContain('overflow-y-auto')
    }
  })
})
