import { describe, expect, it } from 'vitest'

import { mount } from '@vue/test-utils'
import DeckTileBadges from '../DeckTileBadges.vue'

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
  // if it took pointer events; only the control (a popover trigger on the owner's grid) does.
  it('passes pointer events through everywhere except the control', () => {
    const wrapper = mount(DeckTileBadges, {
      props: { legalityStatus: 'banned' },
      slots: { control: CONTROL },
    })

    expect(wrapper.get('div').classes()).toContain('pointer-events-none')
    expect(wrapper.get('.control').element.closest('div')?.classList).toContain(
      'pointer-events-auto',
    )
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
    // `pointer-events` inherits, so the ownership chip's own tooltip is hoverable inside a
    // strip that otherwise passes clicks through to the tile's stretched link.
    expect([...column.classList]).toContain('pointer-events-auto')
  })

  // An empty `#ownership` slot must render no element: a flex `gap` opens between items
  // whether or not they have a size, which would shift the count on every unowned card.
  it('leaves the control alone in the column when there is no ownership content', () => {
    const wrapper = mount(DeckTileBadges, { slots: { control: CONTROL, ownership: '' } })

    const column = wrapper.get('.control').element.parentElement as HTMLElement
    expect(column.children).toHaveLength(1)
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
