import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import DropSection from '../DropSection.vue'

function mountSection(
  drop: { slug: string | null; title: string; card_count: number },
  slots: Record<string, string> = {},
) {
  return mount(DropSection, {
    props: { drop },
    slots: { default: '<div class="grid-body">cards</div>', ...slots },
  })
}

const borderless = { slug: 'borderless', title: 'Borderless', card_count: 12 }
const toggle = (wrapper: ReturnType<typeof mountSection>) => wrapper.find('button[aria-expanded]')
// The grid rides `v-show`, so the section's `<div>` wrapper stays in the DOM and only its
// inline `display` toggles — a direct read of the open/collapsed state (reading the style
// beats `isVisible()`, which mis-handles the empty style left after a re-expand).
const collapsed = (wrapper: ReturnType<typeof mountSection>) =>
  (wrapper.find('section > div').attributes('style') ?? '').includes('display: none')
// The chevron is the only always-visible cue of open/collapsed state (aria-expanded is
// non-visual), so lock in that it rotates: `-rotate-90` points it right when collapsed.
const chevronTurned = (wrapper: ReturnType<typeof mountSection>) =>
  wrapper.find('button svg').classes().includes('-rotate-90')

describe('DropSection', () => {
  it('renders the group title and pluralised card count', () => {
    expect(mountSection(borderless).text()).toContain('Borderless')
    expect(mountSection(borderless).text()).toContain('12 cards')
    expect(mountSection({ slug: 'normal', title: 'Normal', card_count: 1 }).text()).toContain(
      '1 card',
    )
  })

  it('renders trailing header content from the meta slot, beside (not inside) the toggle', () => {
    const wrapper = mountSection(borderless, { meta: '<span class="total">$42.50</span>' })
    // The slotted content lands in the h2 header…
    expect(wrapper.find('h2').text()).toContain('$42.50')
    // …but outside the disclosure button, so it isn't part of the toggle's accessible name.
    expect(wrapper.find('button[aria-expanded]').find('.total').exists()).toBe(false)
  })

  it('renders no meta element when the slot is unprovided', () => {
    // With no meta slot the header holds only the disclosure button — no trailing span.
    expect(mountSection(borderless).find('h2').element.children.length).toBe(1)
  })

  it('anchors the section on the group slug, and omits the id when there is none', () => {
    expect(mountSection(borderless).find('section').attributes('id')).toBe('borderless')
    expect(
      mountSection({ slug: null, title: 'Other', card_count: 3 }).find('section').attributes('id'),
    ).toBeUndefined()
  })

  it('is expanded by default — the grid is shown and the toggle reads open', () => {
    const wrapper = mountSection(borderless)
    expect(toggle(wrapper).attributes('aria-expanded')).toBe('true')
    expect(collapsed(wrapper)).toBe(false)
    expect(chevronTurned(wrapper)).toBe(false)
  })

  it('collapses the grid on click, then re-expands it', async () => {
    const wrapper = mountSection(borderless)

    await toggle(wrapper).trigger('click')
    expect(toggle(wrapper).attributes('aria-expanded')).toBe('false')
    expect(collapsed(wrapper)).toBe(true)
    expect(chevronTurned(wrapper)).toBe(true)

    await toggle(wrapper).trigger('click')
    expect(toggle(wrapper).attributes('aria-expanded')).toBe('true')
    expect(collapsed(wrapper)).toBe(false)
    expect(chevronTurned(wrapper)).toBe(false)
  })
})
