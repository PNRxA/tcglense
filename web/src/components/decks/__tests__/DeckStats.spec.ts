import { describe, expect, it, vi } from 'vitest'
import type { Ref } from 'vue'
import { mount } from '@vue/test-utils'
import DeckStats from '@/components/decks/DeckStats.vue'
import type { DeckAnalytics, DeckSection } from '@/lib/api'

// The panel's numbers are the server's (issue #596), so what's left to test here is the
// two controls that choose *which* question is asked — which sections are the shuffled
// library, and which card the odds are for — plus that the odds slider reads the curve the
// response already carries instead of refetching per tick.

// Capture the params ref the component passes in, so the controls can be asserted through it.
const captured = vi.hoisted(() => ({
  params: null as Ref<{ sections?: number[]; card?: string }> | null,
}))

/** Analytics for a deck whose library is whatever `sections` the request asked for; the
 * stub stands in for the server's fold so the component's own wiring is what's under test. */
function analytics(sections: number[] | undefined): DeckAnalytics {
  const sizes: Record<number, number> = { 1: 60, 2: 15 }
  const chosen = sections ?? [1]
  const librarySize = chosen.reduce((total, id) => total + (sizes[id] ?? 0), 0)
  const composition = (copies: number) => ({
    total_copies: copies,
    unique_cards: copies > 0 ? 1 : 0,
    land_copies: 0,
    average_mana_value: copies > 0 ? 1 : null,
    mana_curve: [{ key: '1', label: '1', count: copies, color: null }],
    colors: [{ key: 'R', label: 'Red', count: copies, color: '#ef4444' }],
    card_types: [{ key: 'Instant', label: 'Instant', count: copies, color: null }],
    card_odds: copies > 0 ? [{ name: 'Lightning Bolt', copies: 4 }] : [],
  })
  return {
    deck: composition(75),
    library: composition(librarySize),
    library_section_ids: chosen,
    default_library_section_ids: [1],
    odds:
      librarySize > 0
        ? {
            name: 'Lightning Bolt',
            copies: 4,
            library_size: librarySize,
            cards_seen: 7,
            at_least_one: 0.4,
            // A curve whose values are trivially checkable: 10%, 20%, … per card seen.
            curve: Array.from({ length: 10 }, (_, index) => (index + 1) / 10),
          }
        : null,
  }
}

vi.mock('@/composables/useDeckAnalysis', async () => {
  const { computed: vueComputed } = await import('vue')
  return {
    useDeckStatsQuery: (
      _game: unknown,
      _deckId: unknown,
      params: Ref<{ sections?: number[]; card?: string }>,
    ) => {
      captured.params = params
      return { data: vueComputed(() => analytics(params.value.sections)) }
    },
    usePublicDeckStatsQuery: () => ({ data: vueComputed(() => analytics(undefined)) }),
  }
})

const SECTIONS: DeckSection[] = [
  { id: 1, name: 'Mainboard', position: 0, is_maybeboard: false },
  { id: 2, name: 'Sideboard', position: 1, is_maybeboard: false },
]

function mountPanel() {
  return mount(DeckStats, { props: { game: 'mtg', deckId: 7, sections: SECTIONS } })
}

describe('DeckStats draw sections', () => {
  it('starts on the library the server picked and lets the viewer widen it', async () => {
    const wrapper = mountPanel()

    // No explicit selection is sent until the viewer makes one — the default library is the
    // server's answer, and asking for it by id would mean reproducing the rule that picks it.
    expect(captured.params!.value.sections).toBeUndefined()
    expect(wrapper.text()).toContain('60 cards from 1 selected section')
    const checkboxes = wrapper.findAll<HTMLInputElement>('input[type="checkbox"]')
    expect(checkboxes).toHaveLength(2)
    expect(checkboxes[0]!.element.checked).toBe(true)
    expect(checkboxes[1]!.element.checked).toBe(false)

    await checkboxes[1]!.setValue(true)
    expect(captured.params!.value.sections).toEqual([1, 2])
    expect(wrapper.text()).toContain('75 cards from 2 selected sections')
  })

  it('selects and deselects every section via the controls', async () => {
    const wrapper = mountPanel()

    const buttons = wrapper.findAll<HTMLButtonElement>('fieldset button')
    const selectAll = buttons.find((button) => button.text() === 'Select all')!
    const deselectAll = buttons.find((button) => button.text() === 'Deselect all')!

    expect(selectAll.element.disabled).toBe(false)
    expect(deselectAll.element.disabled).toBe(false)

    await selectAll.trigger('click')
    const checkboxes = wrapper.findAll<HTMLInputElement>('input[type="checkbox"]')
    expect(checkboxes.every((checkbox) => checkbox.element.checked)).toBe(true)
    expect(wrapper.text()).toContain('75 cards from 2 selected sections')
    expect(selectAll.element.disabled).toBe(true)

    await deselectAll.trigger('click')
    expect(checkboxes.some((checkbox) => checkbox.element.checked)).toBe(false)
    // An empty selection is a real answer, not a fall back to the default.
    expect(captured.params!.value.sections).toEqual([])
    expect(wrapper.text()).toContain('Select at least one section containing cards')
    expect(deselectAll.element.disabled).toBe(true)
  })

  it('resets to the server default when the section list changes underneath it', async () => {
    const wrapper = mountPanel()
    const checkboxes = wrapper.findAll<HTMLInputElement>('input[type="checkbox"]')
    await checkboxes[1]!.setValue(true)
    expect(captured.params!.value.sections).toEqual([1, 2])

    // A renamed section invalidates a selection made against the old list.
    await wrapper.setProps({
      sections: [{ ...SECTIONS[0]!, name: 'Main deck' }, SECTIONS[1]!],
    })
    expect(captured.params!.value.sections).toBeUndefined()
  })
})

describe('DeckStats draw odds', () => {
  it('reads the probability off the curve the response carries', async () => {
    const wrapper = mountPanel()

    // The default is seven cards seen -> curve[6] = 70%.
    expect(wrapper.text()).toContain('70%')
    const slider = wrapper.find<HTMLInputElement>('input[type="range"]')
    expect(slider.attributes('max')).toBe('10')

    await slider.setValue(3)
    expect(wrapper.text()).toContain('30%')
    // Scrubbing the slider must not re-ask the server — the whole curve is already here.
    expect(captured.params!.value).toEqual({ sections: undefined, card: undefined })
  })

  it('renders the composition the server folded', () => {
    const wrapper = mountPanel()
    const text = wrapper.text()
    expect(text).toContain('75')
    expect(text).toContain('Red')
    expect(text).toContain('Instant')
  })
})

describe('DeckStats public mode', () => {
  it('renders from the handle-addressed mirror when given a handle', () => {
    captured.params = null
    const wrapper = mount(DeckStats, {
      props: { game: 'mtg', deckId: 7, sections: SECTIONS, handle: 'alice-0001' },
    })
    // The public hook was selected at mount, so the authed one never ran.
    expect(captured.params).toBeNull()
    expect(wrapper.text()).toContain('60 cards from 1 selected section')
  })
})
