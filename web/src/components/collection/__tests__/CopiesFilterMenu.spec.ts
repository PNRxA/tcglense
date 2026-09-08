import { describe, it, expect, afterEach } from 'vitest'
import { mount } from '@vue/test-utils'
import CopiesFilterMenu from '../CopiesFilterMenu.vue'
import { EMPTY_COPIES_FILTER, type CopiesFilter } from '@/lib/holdingsFilter'

function mountMenu(filter: CopiesFilter = EMPTY_COPIES_FILTER) {
  return mount(CopiesFilterMenu, { props: { filter }, attachTo: document.body })
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0))

/** Open the popover (its content is portalled to <body>). */
async function open(wrapper: ReturnType<typeof mountMenu>) {
  await wrapper.get('button').trigger('click')
  await tick()
}

const byLabel = (label: string) =>
  document.body.querySelector<HTMLInputElement>(`[aria-label="${label}"]`)

/** Type a count into the number box (the form keeps the raw string until Apply). */
async function typeCount(label: string, raw: string) {
  const input = byLabel(label)
  if (!input) throw new Error(`no input labelled "${label}"`)
  input.value = raw
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await tick()
}

/** Click the button whose visible text is exactly `text`. */
async function press(text: string) {
  const button = Array.from(document.body.querySelectorAll<HTMLButtonElement>('button')).find(
    (node) => node.textContent?.trim() === text,
  )
  if (!button) throw new Error(`no button "${text}"`)
  button.click()
  await tick()
}

/** The payload of the last emission of `event`, or undefined if it never fired. */
function lastEmit(wrapper: ReturnType<typeof mountMenu>, event: string): unknown[] | undefined {
  const events = wrapper.emitted(event)
  return events?.[events.length - 1]
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
    const trigger = mountMenu({ min: 5, finish: 'foil' }).get('button')
    expect(trigger.text()).toBe('5 or more foil copies')
    expect(trigger.attributes('aria-pressed')).toBe('true')
    expect(trigger.classes()).toContain('border-primary')
  })

  it('is active on a finish alone', () => {
    expect(mountMenu({ finish: 'foil' }).get('button').text()).toBe('foil copies')
  })

  it('seeds the form from the committed filter', async () => {
    await open(mountMenu({ min: 2, max: 3, finish: 'regular' }))
    // A two-sided range re-opens as "between 2 and 3", regular pressed.
    expect(byLabel('How to compare the count')?.textContent).toContain('Between')
    expect(byLabel('Number of copies')?.value).toBe('2')
    expect(byLabel('Upper number of copies')?.value).toBe('3')
    const pressed = Array.from(
      document.body.querySelectorAll<HTMLElement>(
        '[data-slot="toggle-group-item"][data-state="on"]',
      ),
    ).map((node) => node.textContent?.trim())
    expect(pressed).toEqual(['Regular only'])
  })

  it('applies the typed count through the seeded comparator as one filter', async () => {
    // Seeded "at least 5" (the canonical spelling of ?copies=5-); retype the number.
    const wrapper = mountMenu({ min: 5, finish: 'any' })
    await open(wrapper)
    await typeCount('Number of copies', '7')
    await press('Apply')
    expect(lastEmit(wrapper, 'apply')).toEqual([{ min: 7, finish: 'any' }])
    expect(wrapper.emitted('clear')).toBeUndefined()
  })

  it('applies an exact count and a range through their comparators', async () => {
    const exact = mountMenu({ min: 4, max: 4, finish: 'any' })
    await open(exact)
    await typeCount('Number of copies', '1')
    await press('Apply')
    expect(lastEmit(exact, 'apply')).toEqual([{ min: 1, max: 1, finish: 'any' }])
    document.body.replaceChildren()

    const range = mountMenu({ min: 2, max: 3, finish: 'any' })
    await open(range)
    await typeCount('Number of copies', '1')
    await typeCount('Upper number of copies', '3')
    await press('Apply')
    expect(lastEmit(range, 'apply')).toEqual([{ min: 1, max: 3, finish: 'any' }])
  })

  it('carries the finish toggle into the same single apply', async () => {
    const wrapper = mountMenu({ min: 4, max: 4, finish: 'any' })
    await open(wrapper)
    await press('Foil only')
    await press('Apply')
    // One event carrying both halves — never separate bound / finish writes.
    expect(wrapper.emitted('apply')).toHaveLength(1)
    expect(lastEmit(wrapper, 'apply')).toEqual([{ min: 4, max: 4, finish: 'foil' }])
  })

  it('applies a finish alone when the number is left blank', async () => {
    const wrapper = mountMenu()
    await open(wrapper)
    await press('Foil only')
    await press('Apply')
    expect(lastEmit(wrapper, 'apply')).toEqual([{ finish: 'foil' }])
  })

  it('blocks Apply on a range whose ends are the wrong way round', async () => {
    const wrapper = mountMenu({ min: 2, max: 3, finish: 'any' })
    await open(wrapper)
    await typeCount('Number of copies', '5')
    const apply = Array.from(document.body.querySelectorAll<HTMLButtonElement>('button')).find(
      (node) => node.textContent?.trim() === 'Apply',
    )
    expect(apply?.disabled).toBe(true)
    expect(byLabel('Number of copies')?.getAttribute('aria-invalid')).toBe('true')
  })

  it('offers Clear filter only while something is filtered, and clears with one event', async () => {
    await open(mountMenu())
    expect(document.body.textContent).not.toContain('Clear filter')
    document.body.replaceChildren()

    const wrapper = mountMenu({ min: 2, max: 3, finish: 'foil' })
    await open(wrapper)
    await press('Clear filter')
    expect(wrapper.emitted('clear')).toHaveLength(1)
    expect(wrapper.emitted('apply')).toBeUndefined()
  })
})
