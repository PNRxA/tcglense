import { describe, it, expect } from 'vitest'
import { ref } from 'vue'
import { useSeriesToggle } from '../useSeriesToggle'

describe('useSeriesToggle', () => {
  it('shows every line and can toggle once two carry data', () => {
    const { shownKeys, canToggle, isShown } = useSeriesToggle(ref(['usd', 'usdFoil']))
    expect(shownKeys.value).toEqual(['usd', 'usdFoil'])
    expect(canToggle.value).toBe(true)
    expect(isShown('usd')).toBe(true)
    expect(isShown('usdFoil')).toBe(true)
  })

  it('hides a line on toggle and restores it on a second toggle', () => {
    const { shownKeys, toggle, isShown } = useSeriesToggle(ref(['usd', 'usdFoil']))
    toggle('usdFoil')
    expect(shownKeys.value).toEqual(['usd'])
    expect(isShown('usdFoil')).toBe(false)
    toggle('usdFoil')
    expect(shownKeys.value).toEqual(['usd', 'usdFoil'])
    expect(isShown('usdFoil')).toBe(true)
  })

  it('refuses to hide the last visible line', () => {
    const { shownKeys, toggle } = useSeriesToggle(ref(['usd', 'usdFoil']))
    toggle('usd') // one hidden, one left
    toggle('usdFoil') // would hide the last — ignored
    expect(shownKeys.value).toEqual(['usdFoil'])
  })

  it('never reports fewer than one line: falls back to all when the shown one drops out', () => {
    const dataKeys = ref<string[]>(['usd', 'usdFoil'])
    const { shownKeys, toggle } = useSeriesToggle(dataKeys)
    toggle('usd') // hide cards; sealed stays shown
    expect(shownKeys.value).toEqual(['usdFoil'])
    // A range change leaves only cards with data — the one line the user had hidden. Rather
    // than draw nothing, fall back to showing it.
    dataKeys.value = ['usd']
    expect(shownKeys.value).toEqual(['usd'])
  })

  it('keeps the hidden choice across a data change', () => {
    const dataKeys = ref<string[]>(['usd', 'usdFoil'])
    const { shownKeys, toggle } = useSeriesToggle(dataKeys)
    toggle('usdFoil') // hide sealed
    dataKeys.value = ['usd', 'usdFoil'] // same series reappear (e.g. a different range)
    expect(shownKeys.value).toEqual(['usd']) // still hidden — the choice persists
  })

  it('reports nothing to toggle with fewer than two lines', () => {
    expect(useSeriesToggle(ref(['usd'])).canToggle.value).toBe(false)
    expect(useSeriesToggle(ref([])).canToggle.value).toBe(false)
  })
})
