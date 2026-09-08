import { describe, it, expect, afterEach } from 'vitest'
import { mount } from '@vue/test-utils'
import CopiesFilterMenu from '../CopiesFilterMenu.vue'
import type { HoldingFinish } from '@/lib/holdingsFilter'

function mountMenu(props: { copies?: string; finish?: HoldingFinish } = {}) {
  return mount(CopiesFilterMenu, {
    props: { copies: '', finish: 'any' as HoldingFinish, ...props },
    attachTo: document.body,
  })
}

/** Open the dropdown and hand back the rendered menu items (the content is portalled). */
async function open(wrapper: ReturnType<typeof mountMenu>) {
  await wrapper.get('button').trigger('click')
  await new Promise((resolve) => setTimeout(resolve, 0))
  return Array.from(
    document.body.querySelectorAll<HTMLElement>('[role="menuitemradio"], [role="menuitem"]'),
  )
}

/** The payload of the last emission of `event`, or undefined if it never fired. */
function lastEmit(wrapper: ReturnType<typeof mountMenu>, event: string): unknown[] | undefined {
  const events = wrapper.emitted(event)
  return events?.[events.length - 1]
}

/** Click the menu item whose label is exactly `label`. */
async function pick(wrapper: ReturnType<typeof mountMenu>, label: string) {
  const items = await open(wrapper)
  const item = items.find((node) => node.textContent?.trim() === label)
  if (!item) throw new Error(`no menu item labelled "${label}"`)
  item.click()
  await new Promise((resolve) => setTimeout(resolve, 0))
  await wrapper.vm.$nextTick()
}

describe('CopiesFilterMenu', () => {
  afterEach(() => document.body.replaceChildren())

  it('reads "Copies" and is unhighlighted while nothing is filtered', () => {
    const trigger = mountMenu().get('button')
    expect(trigger.text()).toBe('Copies')
    expect(trigger.attributes('aria-pressed')).toBe('false')
    expect(trigger.classes()).not.toContain('border-primary')
  })

  it('states the active filter in words and wears the active pill', () => {
    const trigger = mountMenu({ copies: '5-', finish: 'foil' }).get('button')
    expect(trigger.text()).toBe('5 or more foil copies')
    expect(trigger.attributes('aria-pressed')).toBe('true')
    expect(trigger.classes()).toContain('border-primary')
  })

  it('is active on a finish alone', () => {
    expect(mountMenu({ finish: 'foil' }).get('button').text()).toBe('foil copies')
  })

  it('offers both groups, with the current values checked', async () => {
    const wrapper = mountMenu({ copies: '4', finish: 'regular' })
    const items = await open(wrapper)
    const labels = items.map((node) => node.textContent?.trim())
    expect(labels).toEqual(
      expect.arrayContaining([
        'Any count',
        '1 copy',
        '2–3 copies',
        'Playset (4)',
        '5 or more',
        'Regular or foil',
        'Regular only',
        'Foil only',
      ]),
    )
    const checked = items
      .filter((node) => node.getAttribute('aria-checked') === 'true')
      .map((node) => node.textContent?.trim())
    expect(checked).toEqual(['Playset (4)', 'Regular only'])
  })

  it('emits the picked copies preset as the ?copies= token', async () => {
    const wrapper = mountMenu()
    await pick(wrapper, 'Playset (4)')
    expect(lastEmit(wrapper, 'update:copies')).toEqual(['4'])
    expect(wrapper.emitted('update:finish')).toBeUndefined()
  })

  it('emits the empty token for "Any count"', async () => {
    const wrapper = mountMenu({ copies: '4' })
    await pick(wrapper, 'Any count')
    expect(lastEmit(wrapper, 'update:copies')).toEqual([''])
  })

  it('emits the picked finish without touching the copies bound', async () => {
    const wrapper = mountMenu({ copies: '4' })
    await pick(wrapper, 'Foil only')
    expect(lastEmit(wrapper, 'update:finish')).toEqual(['foil'])
    expect(wrapper.emitted('update:copies')).toBeUndefined()
  })

  it('keeps the menu open while a radio item is picked, so both groups can be set', async () => {
    const wrapper = mountMenu()
    await pick(wrapper, 'Playset (4)')
    expect(document.body.querySelectorAll('[role="menuitemradio"]').length).toBeGreaterThan(0)
  })

  it('offers Clear filter only while something is filtered, and clears both halves', async () => {
    expect(
      (await open(mountMenu())).some((node) => node.textContent?.includes('Clear filter')),
    ).toBe(false)
    document.body.replaceChildren()

    const wrapper = mountMenu({ copies: '2-3', finish: 'foil' })
    await pick(wrapper, 'Clear filter')
    expect(lastEmit(wrapper, 'update:copies')).toEqual([''])
    expect(lastEmit(wrapper, 'update:finish')).toEqual(['any'])
  })
})
