import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useCardNavStore } from '../cardNav'

describe('card nav store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('reports no position for a card on no registered grid', () => {
    const nav = useCardNavStore()
    nav.register({ game: 'mtg', ids: ['a', 'b', 'c'] })
    expect(nav.locate('mtg', 'z')).toEqual({ prev: null, next: null, index: -1, total: 0 })
  })

  it('finds the neighbours of a card in the middle of its grid', () => {
    const nav = useCardNavStore()
    nav.register({ game: 'mtg', ids: ['a', 'b', 'c'] })
    expect(nav.locate('mtg', 'b')).toEqual({ prev: 'a', next: 'c', index: 1, total: 3 })
  })

  it('has no prev at the first card and no next at the last', () => {
    const nav = useCardNavStore()
    nav.register({ game: 'mtg', ids: ['a', 'b', 'c'] })
    expect(nav.locate('mtg', 'a')).toEqual({ prev: null, next: 'b', index: 0, total: 3 })
    expect(nav.locate('mtg', 'c')).toEqual({ prev: 'b', next: null, index: 2, total: 3 })
  })

  it('reports a lone card with both neighbours null (the modal hides nav for total <= 1)', () => {
    const nav = useCardNavStore()
    nav.register({ game: 'mtg', ids: ['only'] })
    expect(nav.locate('mtg', 'only')).toEqual({ prev: null, next: null, index: 0, total: 1 })
  })

  it('only matches grids for the same game', () => {
    const nav = useCardNavStore()
    nav.register({ game: 'mtg', ids: ['a', 'b'] })
    expect(nav.locate('lorcana', 'a')).toEqual({ prev: null, next: null, index: -1, total: 0 })
  })

  it('picks the first registered grid holding the card (page grid over a later one)', () => {
    const nav = useCardNavStore()
    // The page's own grid registers first; the modal's nested "other printings" grid later.
    nav.register({ game: 'mtg', ids: ['a', 'b', 'c'] })
    nav.register({ game: 'mtg', ids: ['x', 'b', 'y'] })
    // `b` is in both — the earlier (page) grid wins, so nav stays within that list.
    expect(nav.locate('mtg', 'b')).toEqual({ prev: 'a', next: 'c', index: 1, total: 3 })
  })

  it('reflects a grid updating its ids (a page change) in place', () => {
    const nav = useCardNavStore()
    const handle = nav.register({ game: 'mtg', ids: ['a', 'b', 'c'] })
    nav.update(handle, { game: 'mtg', ids: ['d', 'e', 'f'] })
    expect(nav.locate('mtg', 'b')).toEqual({ prev: null, next: null, index: -1, total: 0 })
    expect(nav.locate('mtg', 'e')).toEqual({ prev: 'd', next: 'f', index: 1, total: 3 })
  })

  it('stops offering a grid once it unregisters (unmounts)', () => {
    const nav = useCardNavStore()
    const handle = nav.register({ game: 'mtg', ids: ['a', 'b', 'c'] })
    nav.unregister(handle)
    expect(nav.locate('mtg', 'b')).toEqual({ prev: null, next: null, index: -1, total: 0 })
  })

  it('ignores an update to an already-unregistered handle (no resurrection)', () => {
    const nav = useCardNavStore()
    const handle = nav.register({ game: 'mtg', ids: ['a', 'b'] })
    nav.unregister(handle)
    nav.update(handle, { game: 'mtg', ids: ['a', 'b'] })
    expect(nav.locate('mtg', 'a')).toEqual({ prev: null, next: null, index: -1, total: 0 })
  })
})
