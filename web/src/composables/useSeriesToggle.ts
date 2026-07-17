import { computed, ref, type Ref } from 'vue'

/**
 * Per-line show/hide engine for the value/price chart legend.
 *
 * Given the keys that currently have a plotted point (`dataKeys`), it tracks which the user
 * switched off and derives the set actually drawn. Two guarantees keep the chart honest:
 *
 * - **Never blank.** If the hidden selection ends up covering every plottable line — e.g. a
 *   range change drops the one series the user had left visible — it falls back to showing all
 *   of them rather than draw an empty frame the legend couldn't undo.
 * - **At least one line stays.** `toggle` refuses to hide the final visible line.
 *
 * `hidden` persists across data changes (it's keyed by series identity, not by the current
 * rows), so a choice survives a range switch. Pure and DOM-free, so it unit-tests without the
 * unovis chart body.
 */
export function useSeriesToggle(dataKeys: Ref<readonly string[]>) {
  const hidden = ref<Set<string>>(new Set())

  const shownKeys = computed(() => {
    const shown = dataKeys.value.filter((key) => !hidden.value.has(key))
    return shown.length > 0 ? shown : [...dataKeys.value]
  })

  // There's something to switch between only once two lines carry data.
  const canToggle = computed(() => dataKeys.value.length >= 2)

  const isShown = (key: string) => shownKeys.value.includes(key)

  function toggle(key: string) {
    const next = new Set(hidden.value)
    if (shownKeys.value.includes(key)) {
      if (shownKeys.value.length <= 1) return // keep the last line — never go blank
      next.add(key)
    } else {
      next.delete(key)
    }
    hidden.value = next
  }

  return { hidden, shownKeys, canToggle, isShown, toggle }
}
