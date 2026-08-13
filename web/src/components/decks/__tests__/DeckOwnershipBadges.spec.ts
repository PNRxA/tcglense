import { describe, expect, it } from 'vitest'

import { mount } from '@vue/test-utils'
import DeckOwnershipBadges from '../DeckOwnershipBadges.vue'

describe('DeckOwnershipBadges', () => {
  it('shows a chip per non-zero holding, collection before wish list', () => {
    const wrapper = mount(DeckOwnershipBadges, { props: { owned: 2, wanted: 3 } })

    const chips = wrapper.findAll('span')
    expect(chips.map((c) => c.text())).toEqual(['2', '3'])
    expect(chips[0]?.attributes('title')).toBe('You own 2 of this card')
    expect(chips[1]?.attributes('title')).toBe('You have 3 of this card on your wish list')
  })

  // The chip's text is a bare number and its icon is `aria-hidden`, so without a name of its
  // own a screen reader hears "2" — and on a card tile these now precede the deck count.
  it('names each chip for a screen reader rather than relying on the mouse-only title', () => {
    const wrapper = mount(DeckOwnershipBadges, { props: { owned: 2, wanted: 3 } })

    const chips = wrapper.findAll('span')
    expect(chips.map((c) => c.attributes('role'))).toEqual(['img', 'img'])
    expect(chips.map((c) => c.attributes('aria-label'))).toEqual([
      'You own 2 of this card',
      'You have 3 of this card on your wish list',
    ])
  })

  it('drops the chip for a holding that is zero', () => {
    const owned = mount(DeckOwnershipBadges, { props: { owned: 4, wanted: 0 } })
    expect(owned.findAll('span')).toHaveLength(1)
    expect(owned.get('span').attributes('title')).toBe('You own 4 of this card')

    const wanted = mount(DeckOwnershipBadges, { props: { owned: 0, wanted: 1 } })
    expect(wanted.findAll('span')).toHaveLength(1)
    expect(wanted.get('span').attributes('title')).toBe('You have 1 of this card on your wish list')
  })

  // Load-bearing for the image grid: the chips share a flex column with the deck count, and a
  // `gap` opens between items whether or not they have a size — so a wrapper element rendered
  // for a card the viewer neither owns nor wants would push the count up on most of the grid.
  it('renders no element at all when the viewer neither owns nor wants the card', () => {
    const wrapper = mount(DeckOwnershipBadges, { props: { owned: 0, wanted: 0 } })

    expect(wrapper.find('div').exists()).toBe(false)
    expect(wrapper.html()).toBe('<!--v-if-->')
  })
})
