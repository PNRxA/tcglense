import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import OwnedCountBadge from '../OwnedCountBadge.vue'

// The badge is rendered inside OwnedCountControl's trigger with `tooltip={false}` (a hover
// tooltip would fight the click-to-open popover), so the tests exercise that branch.
function mountBadge(props: { quantity: number; foilQuantity: number; hoverAsAdd?: boolean }) {
  return mount(OwnedCountBadge, { props: { tooltip: false, ...props } })
}

// The chips carry a semantic `aria-label` ("3 total" / "1 foil").
function chip(wrapper: ReturnType<typeof mountBadge>, label: string) {
  return wrapper.find(`span[aria-label="${label}"]`)
}

// Count SVG icons whose class list includes a given (literal) Tailwind class — the plus
// swap tags its icons with distinct group-hover/focus classes we can assert on.
function iconsWithClass(wrapper: ReturnType<typeof mountBadge>, cls: string) {
  return wrapper.findAll('svg').filter((s) => s.classes().includes(cls))
}

describe('OwnedCountBadge', () => {
  it('shows a total chip, plus a foil chip only when some copies are foil', () => {
    const wrapper = mountBadge({ quantity: 2, foilQuantity: 1 })
    // Total is regular + foil (3); the foil chip counts just the foils (1).
    expect(chip(wrapper, '3 total').exists()).toBe(true)
    expect(chip(wrapper, '3 total').text()).toContain('3')
    expect(chip(wrapper, '1 foil').exists()).toBe(true)
  })

  it('omits the foil chip when nothing owned is foil', () => {
    const wrapper = mountBadge({ quantity: 3, foilQuantity: 0 })
    expect(chip(wrapper, '3 total').exists()).toBe(true)
    expect(wrapper.find('span[aria-label$="foil"]').exists()).toBe(false)
  })

  it('renders nothing when the card is not owned', () => {
    const wrapper = mountBadge({ quantity: 0, foilQuantity: 0 })
    expect(wrapper.findAll('span[aria-label]')).toHaveLength(0)
  })

  describe('hover-as-add (issue #136)', () => {
    it('renders no plus-swap icons by default', () => {
      const wrapper = mountBadge({ quantity: 2, foilQuantity: 1 })
      expect(iconsWithClass(wrapper, 'group-hover/add:block')).toHaveLength(0)
      expect(iconsWithClass(wrapper, 'group-hover/add:hidden')).toHaveLength(0)
    })

    it('adds a hover-revealed "+" to each chip while keeping its count', () => {
      const wrapper = mountBadge({ quantity: 2, foilQuantity: 1, hoverAsAdd: true })
      // One "+" (shown on group hover/focus) and one semantic icon (hidden then) per chip —
      // here two chips: total and foil.
      expect(iconsWithClass(wrapper, 'group-hover/add:block')).toHaveLength(2)
      expect(iconsWithClass(wrapper, 'group-hover/add:hidden')).toHaveLength(2)
      // The counts are untouched by the swap.
      expect(chip(wrapper, '3 total').text()).toContain('3')
      expect(chip(wrapper, '1 foil').text()).toContain('1')
    })

    it('swaps only the single chip when the card is owned in regular only', () => {
      const wrapper = mountBadge({ quantity: 4, foilQuantity: 0, hoverAsAdd: true })
      expect(iconsWithClass(wrapper, 'group-hover/add:block')).toHaveLength(1)
    })
  })
})
