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

// The component re-reads every rect on each event, so `setTop` moves a heading the way scrolling
// would. A target frozen at its mount-time position would let the state computed on mount pass
// for tracking, whether or not a listener is still wired up.
function addTarget(sectionId: number, top: number) {
  const target = document.createElement('section')
  target.id = deckSectionTargetId(sectionId)
  let currentTop = top
  target.getBoundingClientRect = vi.fn<() => DOMRect>(() => ({ top: currentTop }) as DOMRect)
  const scrollIntoView = vi.fn<() => void>()
  target.scrollIntoView = scrollIntoView
  document.body.append(target)
  return {
    target,
    scrollIntoView,
    setTop: (next: number) => {
      currentTop = next
    },
  }
}

// jsdom lays nothing out, so the document height the component measures against has to be stated
// outright; the own property shadows the prototype getter until afterEach removes it.
function stubScrollHeight(scrollHeight: number) {
  Object.defineProperty(document.documentElement, 'scrollHeight', {
    value: scrollHeight,
    configurable: true,
  })
}

afterEach(() => {
  document.body.replaceChildren()
  Reflect.deleteProperty(document.documentElement, 'scrollHeight')
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
    const { target, scrollIntoView } = addTarget(30, 600)
    const wrapper = mount(DeckSectionNav, { props: { items } })

    await wrapper.find('select').setValue('30')

    expect(scrollIntoView).toHaveBeenCalledExactlyOnceWith({
      behavior: 'smooth',
      block: 'start',
    })
    expect(document.activeElement).toBe(target)
  })

  it('moves focus onto the category it jumped to', async () => {
    const { target } = addTarget(20, 400)
    const wrapper = mount(DeckSectionNav, { props: { items } })

    await wrapper.findAll('a')[1]!.trigger('click')

    // Focus is what carries a keyboard user's next Tab, and a screen reader's cursor, out of the
    // sidebar and into the section the link scrolled to.
    expect(document.activeElement).toBe(target)
  })

  it('tracks the category at the upper part of the viewport while scrolling', async () => {
    const creatures = addTarget(10, 40)
    const removal = addTarget(20, 500)
    addTarget(30, 900)
    const wrapper = mount(DeckSectionNav, { props: { items } })
    expect(wrapper.findAll('a')[0]!.attributes('aria-current')).toBe('location')

    creatures.setTop(-400)
    removal.setTop(40)
    window.dispatchEvent(new Event('scroll'))
    await wrapper.vm.$nextTick()

    expect(wrapper.findAll('a')[1]!.attributes('aria-current')).toBe('location')
    expect((wrapper.find('select').element as HTMLSelectElement).value).toBe('20')
  })

  it('re-tracks the current category when a resize moves the marker', async () => {
    // The marker sits a quarter of the way down the viewport, so a taller window reaches a
    // heading that a short one leaves below it.
    vi.stubGlobal('innerHeight', 200)
    addTarget(10, -400)
    addTarget(20, 100)
    addTarget(30, 900)
    const wrapper = mount(DeckSectionNav, { props: { items } })
    expect(wrapper.findAll('a')[0]!.attributes('aria-current')).toBe('location')

    vi.stubGlobal('innerHeight', 800)
    window.dispatchEvent(new Event('resize'))
    await wrapper.vm.$nextTick()

    expect(wrapper.findAll('a')[1]!.attributes('aria-current')).toBe('location')
  })

  it('selects a short final category once the page has scrolled to its end', async () => {
    // A final section shorter than the trailing padding and footer beneath it keeps its heading
    // below the marker at maximum scroll, where no further scrolling can bring it up.
    addTarget(10, -1200)
    addTarget(20, -600)
    addTarget(30, 254)
    vi.stubGlobal('innerHeight', 900)
    vi.stubGlobal('scrollY', 1100)
    stubScrollHeight(2000)
    const wrapper = mount(DeckSectionNav, { props: { items } })

    window.dispatchEvent(new Event('scroll'))
    await wrapper.vm.$nextTick()

    expect(wrapper.findAll('a')[2]!.attributes('aria-current')).toBe('location')
    expect((wrapper.find('select').element as HTMLSelectElement).value).toBe('30')
  })

  it('honours reduced-motion preferences when jumping', async () => {
    vi.stubGlobal('matchMedia', vi.fn().mockReturnValue({ matches: true }))
    const { scrollIntoView } = addTarget(20, 400)
    const wrapper = mount(DeckSectionNav, { props: { items } })

    await wrapper.findAll('a')[1]!.trigger('click')

    expect(scrollIntoView).toHaveBeenCalledWith({ behavior: 'auto', block: 'start' })
  })
})
