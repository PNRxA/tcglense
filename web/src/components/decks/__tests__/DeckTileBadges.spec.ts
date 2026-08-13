import { describe, expect, it } from 'vitest'

import { h } from 'vue'
import { mount } from '@vue/test-utils'
import DeckTileBadges from '../DeckTileBadges.vue'
import DeckOwnershipBadges from '../DeckOwnershipBadges.vue'

const CONTROL = '<span class="control">×2</span>'

describe('DeckTileBadges', () => {
  // The two chips used to be pinned to opposite corners of the tile, which only kept them
  // apart while they both fit — a foil holding's second count chip and "Over Limit" landed on
  // top of each other. Sitting in one flex line is what makes overlapping impossible, and
  // wrapping upwards is what a too-narrow tile does instead of clipping the pair.
  it('lays the control and the legality chip out in one wrapping line', () => {
    const wrapper = mount(DeckTileBadges, {
      props: { legalityStatus: 'over_limit' },
      slots: { control: CONTROL },
    })

    const strip = wrapper.get('div')
    expect(strip.classes()).toEqual(
      expect.arrayContaining([
        'flex',
        'flex-wrap-reverse',
        'absolute',
        'bottom-1.5',
        'inset-x-1.5',
      ]),
    )
    const chip = wrapper.get('span:not(.control)')
    expect(chip.text()).toBe('Over Limit')
    // Both are children of that one line — neither is positioned against the tile itself.
    expect(wrapper.get('.control').element.closest('div')?.parentElement).toBe(strip.element)
    expect(chip.element.parentElement).toBe(strip.element)
    expect(chip.classes()).not.toContain('absolute')
  })

  // The strip spans the tile, so it would swallow clicks meant for the tile's stretched link
  // if it took pointer events. The *chips* take theirs back, not the column holding them: the
  // column's box is as wide as its widest row and as tall as both, so re-enabling events on it
  // handed a rectangle of bare artwork to a div at z-20 — measured live, a click in the slack
  // beside a one-chip count hit that div instead of the card link.
  it('gives pointer events back to the chips themselves, never the column around them', () => {
    const wrapper = mount(DeckTileBadges, {
      props: { legalityStatus: 'banned' },
      slots: { control: CONTROL },
    })

    expect(wrapper.get('div').classes()).toContain('pointer-events-none')
    const column = wrapper.get('.control').element.closest('div') as HTMLElement
    expect([...column.classList]).toContain('[&>*]:pointer-events-auto')
    expect([...column.classList]).not.toContain('pointer-events-auto')
  })

  // Ownership used to be pinned to the tile's top-right corner, diagonally opposite the deck
  // count it qualifies. Both answer "how many?", so they stack in the one bottom-left column.
  it('stacks the ownership chips directly above the control, in one column', () => {
    const wrapper = mount(DeckTileBadges, {
      slots: { control: CONTROL, ownership: '<span class="owned">2</span>' },
    })

    const column = wrapper.get('.owned').element.parentElement as HTMLElement
    expect(column).toBe(wrapper.get('.control').element.parentElement)
    expect([...column.classList]).toEqual(
      expect.arrayContaining(['flex', 'flex-col', 'items-start', 'mr-auto']),
    )
    // Ownership first in a column that flows downwards = above the count.
    expect([...column.children].indexOf(wrapper.get('.owned').element)).toBe(0)
  })

  // `flex-wrap-reverse` permutes the cross axis, so the value that means "sit on the tile's
  // bottom edge" is `items-start`. With `items-end` the trailing chip pinned to the *top* of
  // the line — invisible while the left slot was a single chip, but once it became a column
  // "Over Limit" floated 24px up to sit level with the ownership chips instead of the count
  // (measured in a browser: 30px above the tile's bottom edge against the control's 6px).
  it('aligns the trailing chip to the tile’s bottom edge, not the top of the line', () => {
    const wrapper = mount(DeckTileBadges, {
      props: { legalityStatus: 'over_limit' },
      slots: { control: CONTROL, ownership: '<span class="owned">2</span>' },
    })

    const strip = wrapper.get('div')
    expect(strip.classes()).toContain('items-start')
    expect(strip.classes()).not.toContain('items-end')
  })

  // An empty `#ownership` slot must render no element: a flex `gap` opens between items
  // whether or not they have a size, which would shift the count on every unowned card. Mount
  // the real component at 0/0 rather than an empty string — the guarantee is DeckOwnershipBadges'
  // own `v-if`, and an empty-string slot would pass against any version of either component.
  it('leaves the control alone in the column when the viewer owns and wants none', () => {
    const wrapper = mount(DeckTileBadges, {
      slots: {
        control: CONTROL,
        ownership: () => h(DeckOwnershipBadges, { owned: 0, wanted: 0 }),
      },
    })

    const column = wrapper.get('.control').element.parentElement as HTMLElement
    expect(column.children).toHaveLength(1)
    expect(wrapper.findComponent(DeckOwnershipBadges).exists()).toBe(true)
  })

  it('renders no trailing chip for a legal card, and a caller-supplied one when given', () => {
    const legal = mount(DeckTileBadges, { slots: { control: CONTROL } })
    expect(legal.findAll('span')).toHaveLength(1)

    // The precon page's own trailing chip ("N foil") replaces the legality one.
    const precon = mount(DeckTileBadges, {
      props: { legalityStatus: 'over_limit' },
      slots: { control: CONTROL, trailing: '<span class="foil">2 foil</span>' },
    })
    expect(precon.get('.foil').text()).toBe('2 foil')
    expect(precon.text()).not.toContain('Over Limit')
  })
})
