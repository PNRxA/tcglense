import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import { Heart, Layers } from '@lucide/vue'
import OwnedCountBadge from '../OwnedCountBadge.vue'

// The badge is rendered inside OwnedCountControl's trigger with `tooltip={false}` (a hover
// tooltip would fight the click-to-open popover), so the tests exercise that branch.
function mountBadge(props: {
  quantity: number
  foilQuantity: number
  hoverAsAdd?: boolean
  wantedQuantity?: number
  kind?: 'owned' | 'wanted'
}) {
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

  describe('heart wishlist chip (issue #364)', () => {
    it("leads the total chip with a heart under kind='wanted'", () => {
      const wrapper = mountBadge({ quantity: 2, foilQuantity: 1, kind: 'wanted' })
      // The total chip reads "wanted" (heart-led); the foil chip is unchanged; no "total" chip.
      expect(chip(wrapper, '3 wanted').exists()).toBe(true)
      expect(chip(wrapper, '1 foil').exists()).toBe(true)
      expect(chip(wrapper, '3 total').exists()).toBe(false)
    })

    it('appends a "wanted" chip after total/foil on an owned + wish-listed badge', () => {
      const wrapper = mountBadge({ quantity: 3, foilQuantity: 0, wantedQuantity: 2 })
      expect(chip(wrapper, '3 total').exists()).toBe(true)
      expect(chip(wrapper, '2 wanted').exists()).toBe(true)
      // The heart chip sits to the RIGHT of the total chip.
      const labels = wrapper.findAll('span[aria-label]').map((s) => s.attributes('aria-label'))
      expect(labels.indexOf('2 wanted')).toBeGreaterThan(labels.indexOf('3 total'))
    })

    it('never swaps the informational wanted chip to a "+" under hoverAsAdd', () => {
      const wrapper = mountBadge({
        quantity: 3,
        foilQuantity: 0,
        wantedQuantity: 2,
        hoverAsAdd: true,
      })
      // Only the total chip swaps; the heart chip contributes no "+".
      expect(iconsWithClass(wrapper, 'group-hover/add:block')).toHaveLength(1)
      expect(chip(wrapper, '2 wanted').text()).toContain('2')
    })

    it("ignores wantedQuantity under kind='wanted' (no duplicate heart chip)", () => {
      const wrapper = mountBadge({
        quantity: 3,
        foilQuantity: 0,
        kind: 'wanted',
        wantedQuantity: 5,
      })
      // Just the heart-led total; the wantedQuantity is ignored in wanted mode.
      expect(wrapper.findAll('span[aria-label$="wanted"]')).toHaveLength(1)
    })
  })

  // The rendered leading ICON per chip, not just its aria-label — the label and the icon are
  // picked independently in the component, so an inverted icon ternary would leave every
  // label-based assertion above green. Assert the actual Heart / Layers component identity.
  describe('chip leading icon identity (issue #364)', () => {
    it("leads the total chip with a Heart (not Layers) under kind='wanted'", () => {
      const wrapper = mountBadge({ quantity: 2, foilQuantity: 1, kind: 'wanted' })
      const hearts = wrapper.findAllComponents(Heart)
      expect(hearts).toHaveLength(1)
      expect(wrapper.findAllComponents(Layers)).toHaveLength(0)
      // The lone heart is the leading icon of the "3 wanted" total chip.
      expect(chip(wrapper, '3 wanted').element.contains(hearts[0]!.element)).toBe(true)
    })

    it('leads the total chip with Layers (not Heart) under the default kind', () => {
      const wrapper = mountBadge({ quantity: 2, foilQuantity: 1 })
      const layers = wrapper.findAllComponents(Layers)
      expect(layers).toHaveLength(1)
      expect(wrapper.findAllComponents(Heart)).toHaveLength(0)
      expect(chip(wrapper, '3 total').element.contains(layers[0]!.element)).toBe(true)
    })

    it('leads the appended wanted chip with a Heart while the total chip stays Layers', () => {
      const wrapper = mountBadge({ quantity: 3, foilQuantity: 0, wantedQuantity: 2 })
      const hearts = wrapper.findAllComponents(Heart)
      const layers = wrapper.findAllComponents(Layers)
      expect(hearts).toHaveLength(1)
      expect(layers).toHaveLength(1)
      // Layers leads the total chip; the Heart leads the appended informational wanted chip.
      expect(chip(wrapper, '3 total').element.contains(layers[0]!.element)).toBe(true)
      expect(chip(wrapper, '2 wanted').element.contains(hearts[0]!.element)).toBe(true)
    })
  })
})
