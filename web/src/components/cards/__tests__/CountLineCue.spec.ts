import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { Loader2, TriangleAlert } from '@lucide/vue'
import CountLineCue from '../CountLineCue.vue'
import StaleNotice from '../StaleNotice.vue'
import UpdatingCue from '../UpdatingCue.vue'

// `CountLineCue` exists so the icon metrics live in one place: `UpdatingCue` (work in flight)
// and `StaleNotice` (a background refetch failed, issue #622) can share a count line, and a cue
// whose icon is a pixel bigger than its neighbour's reads as a rendering bug. These pin that the
// two wrappers really do go through the seam rather than carrying their own copy.
const ICON_CLASSES = ['mr-1', 'inline', 'size-3.5', 'align-[-0.15em]']

describe('CountLineCue', () => {
  it('renders the icon before the label, with the icon hidden from assistive tech', () => {
    const wrapper = mount(CountLineCue, { props: { icon: TriangleAlert, label: 'Stale.' } })
    const svg = wrapper.find('svg')

    expect(svg.exists()).toBe(true)
    expect(svg.attributes('aria-hidden')).toBe('true')
    expect(wrapper.text()).toBe('Stale.')
    for (const cls of ICON_CLASSES) expect(svg.classes()).toContain(cls)
  })

  it('spins only when asked', () => {
    const still = mount(CountLineCue, { props: { icon: TriangleAlert, label: 'x' } })
    expect(still.find('svg').classes()).not.toContain('animate-spin')

    const spinning = mount(CountLineCue, { props: { icon: Loader2, label: 'x', spin: true } })
    expect(spinning.find('svg').classes()).toContain('animate-spin')
  })
})

describe('UpdatingCue', () => {
  it('keeps its spinner, its default label, and the shared metrics', () => {
    const wrapper = mount(UpdatingCue)
    const svg = wrapper.find('svg')

    expect(wrapper.text()).toBe('Updating…')
    expect(svg.classes()).toContain('animate-spin')
    for (const cls of ICON_CLASSES) expect(svg.classes()).toContain(cls)
  })

  it('lets a caller name the work', () => {
    expect(mount(UpdatingCue, { props: { label: 'Dealing…' } }).text()).toBe('Dealing…')
  })
})

describe('StaleNotice', () => {
  it('names what went stale and announces politely without a spinner', () => {
    const wrapper = mount(StaleNotice, { props: { label: "Couldn't refresh — showing decks." } })

    expect(wrapper.text()).toBe("Couldn't refresh — showing decks.")
    expect(wrapper.find('p').attributes('aria-live')).toBe('polite')
    expect(wrapper.find('svg').classes()).not.toContain('animate-spin')
    for (const cls of ICON_CLASSES) expect(wrapper.find('svg').classes()).toContain(cls)
  })

  it('falls back to a generic line', () => {
    expect(mount(StaleNotice).text()).toContain("Couldn't refresh")
  })
})
