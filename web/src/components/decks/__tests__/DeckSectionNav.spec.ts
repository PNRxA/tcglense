import { afterEach, describe, expect, it, vi } from 'vitest'
import { enableAutoUnmount, mount } from '@vue/test-utils'
import DeckSectionNav from '../DeckSectionNav.vue'
import { deckSectionTargetId } from '@/lib/deckSectionNav'

const items = [
  { id: 10, name: 'Creatures', count: 24 },
  { id: 20, name: 'Removal', count: 8 },
  { id: 30, name: 'Lands', count: 36 },
]

enableAutoUnmount(afterEach)

function addTarget(sectionId: number, top: number) {
  const target = document.createElement('section')
  target.id = deckSectionTargetId(sectionId)
  target.getBoundingClientRect = vi.fn<() => DOMRect>(() => ({ top }) as DOMRect)
  const scrollIntoView = vi.fn<() => void>()
  target.scrollIntoView = scrollIntoView
  document.body.append(target)
  return { target, scrollIntoView }
}

afterEach(() => {
  document.body.replaceChildren()
  vi.unstubAllGlobals()
})

describe('DeckSectionNav', () => {
  it('renders the category counts in desktop and mobile navigation', () => {
    const wrapper = mount(DeckSectionNav, { props: { items } })

    expect(wrapper.findAll('a').map((link) => link.text())).toEqual([
      'Creatures24',
      'Removal8',
      'Lands36',
    ])
    expect(wrapper.findAll('option').map((option) => option.text())).toEqual([
      'Creatures (24)',
      'Removal (8)',
      'Lands (36)',
    ])
    expect(wrapper.find('a').attributes('aria-current')).toBe('location')
  })

  it('smoothly jumps to a category from the desktop sidebar', async () => {
    const { scrollIntoView } = addTarget(20, 400)
    const wrapper = mount(DeckSectionNav, { props: { items } })

    await wrapper.findAll('a')[1]!.trigger('click')

    expect(scrollIntoView).toHaveBeenCalledExactlyOnceWith({
      behavior: 'smooth',
      block: 'start',
    })
  })

  it('uses the compact picker to jump on mobile', async () => {
    const { scrollIntoView } = addTarget(30, 600)
    const wrapper = mount(DeckSectionNav, { props: { items } })

    await wrapper.find('select').setValue('30')

    expect(scrollIntoView).toHaveBeenCalledExactlyOnceWith({
      behavior: 'smooth',
      block: 'start',
    })
  })

  it('tracks the category at the upper part of the viewport while scrolling', async () => {
    addTarget(10, -400)
    addTarget(20, 40)
    addTarget(30, 500)
    const wrapper = mount(DeckSectionNav, { props: { items } })

    window.dispatchEvent(new Event('scroll'))
    await wrapper.vm.$nextTick()

    expect(wrapper.findAll('a')[1]!.attributes('aria-current')).toBe('location')
    expect((wrapper.find('select').element as HTMLSelectElement).value).toBe('20')
  })

  it('honours reduced-motion preferences when jumping', async () => {
    vi.stubGlobal('matchMedia', vi.fn().mockReturnValue({ matches: true }))
    const { scrollIntoView } = addTarget(20, 400)
    const wrapper = mount(DeckSectionNav, { props: { items } })

    await wrapper.findAll('a')[1]!.trigger('click')

    expect(scrollIntoView).toHaveBeenCalledWith({ behavior: 'auto', block: 'start' })
  })
})
