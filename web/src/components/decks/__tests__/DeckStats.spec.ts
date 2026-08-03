import { beforeEach, describe, expect, it, vi } from 'vitest'
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

/** The stub server: one section per row, each contributing its own size and its own card.
 * Section-aware on purpose — every case below drives it through the panel's real controls
 * rather than poking a knob the component can't see. */
const SECTION_CARDS: Record<number, { size: number; card: string }> = {
  1: { size: 60, card: 'Lightning Bolt' },
  2: { size: 15, card: 'Pyroblast' },
  3: { size: 2, card: 'Black Lotus' },
}

/** Analytics for the sections the request asked for; stands in for the server's fold so the
 * component's own wiring is what's under test. Mirrors the two server behaviours the panel
 * depends on: the odds curve runs to `min(30, library)`, and an unknown `?card=` falls back
 * to the most-copied one (`analyse_stats`). */
function analytics(sections: number[] | undefined, card: string | undefined): DeckAnalytics {
  const chosen = sections ?? [1]
  const librarySize = chosen.reduce((total, id) => total + (SECTION_CARDS[id]?.size ?? 0), 0)
  const odds_ = chosen
    .map((id) => SECTION_CARDS[id])
    .filter((entry): entry is { size: number; card: string } => entry != null)
    .map((entry) => ({ name: entry.card, copies: entry.size }))
    .sort((left, right) => right.copies - left.copies)
  const composition = (copies: number, cardOdds: typeof odds_) => ({
    total_copies: copies,
    unique_cards: cardOdds.length,
    land_copies: 0,
    average_mana_value: copies > 0 ? 1 : null,
    mana_curve: [{ key: '1', label: '1', count: copies, color: null }],
    colors: [{ key: 'R', label: 'Red', count: copies, color: '#ef4444' }],
    card_types: [{ key: 'Instant', label: 'Instant', count: copies, color: null }],
    card_odds: cardOdds,
  })
  const length = Math.min(30, librarySize)
  const selected = odds_.find((item) => item.name === card) ?? odds_[0]
  return {
    deck: composition(75, [{ name: 'Lightning Bolt', copies: 4 }]),
    library: composition(librarySize, odds_),
    library_section_ids: chosen,
    default_library_section_ids: [1],
    odds:
      selected && length > 0
        ? {
            name: selected.name,
            copies: selected.copies,
            library_size: librarySize,
            cards_seen: 7,
            at_least_one: 0.4,
            // Evenly spaced, so a probability reads straight off the index.
            curve: Array.from({ length }, (_, index) => (index + 1) / length),
          }
        : null,
  }
}

/** The query's own state, read at mount — each loading case mounts its own panel, the way
 * the goldfish spec's `failNext` does. */
const query = vi.hoisted(() => ({ pending: false, fetching: false, failed: false }))

vi.mock('@/composables/useDeckAnalysis', async () => {
  const { computed: vueComputed } = await import('vue')
  const flags = () => ({
    isPending: vueComputed(() => query.pending),
    isFetching: vueComputed(() => query.fetching || query.pending),
    isError: vueComputed(() => query.failed),
  })
  const answer = (sections: number[] | undefined, card: string | undefined) =>
    query.pending || query.failed ? undefined : analytics(sections, card)
  return {
    useDeckStatsQuery: (
      _game: unknown,
      _deckId: unknown,
      params: Ref<{ sections?: number[]; card?: string }>,
    ) => {
      captured.params = params
      return {
        data: vueComputed(() => answer(params.value.sections, params.value.card)),
        ...flags(),
      }
    },
    usePublicDeckStatsQuery: () => ({
      data: vueComputed(() => answer(undefined, undefined)),
      ...flags(),
    }),
  }
})

beforeEach(() => {
  query.pending = false
  query.fetching = false
  query.failed = false
})

const SECTIONS: DeckSection[] = [
  { id: 1, name: 'Mainboard', position: 0, is_maybeboard: false },
  { id: 2, name: 'Sideboard', position: 1, is_maybeboard: false },
  { id: 3, name: 'Tiny', position: 2, is_maybeboard: false },
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
    expect(checkboxes).toHaveLength(3)
    expect(checkboxes[0]!.element.checked).toBe(true)
    expect(checkboxes[1]!.element.checked).toBe(false)
    expect(checkboxes[2]!.element.checked).toBe(false)

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
    expect(wrapper.text()).toContain('77 cards from 3 selected sections')
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
      sections: [{ ...SECTIONS[0]!, name: 'Main deck' }, SECTIONS[1]!, SECTIONS[2]!],
    })
    expect(captured.params!.value.sections).toBeUndefined()
  })
})

describe('DeckStats draw odds', () => {
  it('reads the probability off the curve the response carries', async () => {
    const wrapper = mountPanel()

    // The default library is 60 cards, so the curve runs to 30 (the server's cap) and the
    // default seven cards seen reads curve[6] = 7/30.
    expect(wrapper.text()).toContain('23.3%')
    const slider = wrapper.find<HTMLInputElement>('input[type="range"]')
    expect(slider.attributes('max')).toBe('30')

    await slider.setValue(3)
    expect(wrapper.text()).toContain('10%')
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

describe('DeckStats loading states', () => {
  it('draws its own shape while the first numbers are in flight', () => {
    query.pending = true
    const wrapper = mountPanel()

    // The panel is present and says what it is doing, rather than the card being absent
    // until the response lands and then shoving the deck down the page.
    expect(wrapper.text()).toContain('Deck analytics')
    expect(wrapper.text()).toContain('Crunching numbers…')
    expect(wrapper.find('[data-slot="card"]').attributes('aria-busy')).toBe('true')
    expect(wrapper.findAll('[data-slot="skeleton"]').length).toBeGreaterThan(0)
    // No numbers are invented for the skeleton.
    expect(wrapper.find('input[type="range"]').exists()).toBe(false)
  })

  it('says so when the numbers cannot be worked out', () => {
    query.failed = true
    const wrapper = mountPanel()
    expect(wrapper.text()).toContain("couldn't be worked out")
    expect(wrapper.find('input[type="range"]').exists()).toBe(false)
  })

  it('marks the previous answer as being replaced while a new one is in flight', () => {
    query.fetching = true
    const wrapper = mountPanel()

    // keepPreviousData deliberately leaves the last answer on screen, so the cue is the
    // only thing distinguishing "your click did nothing" from "your click is in the air".
    expect(wrapper.text()).toContain('Recalculating…')
    expect(wrapper.find('[data-slot="card"]').attributes('aria-busy')).toBe('true')
    // The count line describes the *previous* selection while the next is in flight.
    expect(wrapper.text()).not.toContain('60 cards from 1 selected section')
    expect(wrapper.text()).toContain('Updating…')
    // The controls stay live — they are what addresses the next request.
    expect(wrapper.find('input[type="range"]').exists()).toBe(true)
  })

  it('shows no cue at rest', () => {
    const wrapper = mountPanel()
    expect(wrapper.text()).not.toContain('Recalculating…')
    expect(wrapper.text()).not.toContain('Updating…')
    expect(wrapper.find('[data-slot="card"]').attributes('aria-busy')).toBeUndefined()
  })
})

describe('DeckStats stale selections', () => {
  it('clamps "cards seen" at read time rather than writing it back', async () => {
    // A shorter curve must not leave the slider pointing past the end of it — that read
    // `undefined` and rendered 0% for a card certain to be drawn — and the viewer's position
    // must come back when the library grows again.
    const wrapper = mountPanel()
    const boxes = () => wrapper.findAll<HTMLInputElement>('input[type="checkbox"]')
    const slider = () => wrapper.find<HTMLInputElement>('input[type="range"]')

    // Default library is Mainboard's 60, so the curve runs to 30.
    expect(slider().attributes('max')).toBe('30')
    await slider().setValue(9)
    expect(wrapper.text()).toContain('30%')

    // Narrow it to the 2-card section: max drops to 2, so 9 reads as 2 — not past the end.
    await boxes()[0]!.setValue(false)
    await boxes()[2]!.setValue(true)
    expect(slider().attributes('max')).toBe('2')
    expect(wrapper.text()).toContain('100%')

    // Widen it again and the viewer's 9 is still their 9.
    await boxes()[0]!.setValue(true)
    await boxes()[2]!.setValue(false)
    expect(slider().attributes('max')).toBe('30')
    expect(wrapper.text()).toContain('30%')
  })

  it('stops asking about a card that has left the library', async () => {
    const wrapper = mountPanel()
    const boxes = () => wrapper.findAll<HTMLInputElement>('input[type="checkbox"]')

    // Add the Sideboard, then pick the card only it contributes.
    await boxes()[1]!.setValue(true)
    ;(wrapper.vm as unknown as { selectedCard: string }).selectedCard = 'Pyroblast'
    await wrapper.vm.$nextTick()
    expect(captured.params!.value.card).toBe('Pyroblast')

    // Remove the section it came from. The select must re-follow the server's fallback —
    // otherwise it names a card that isn't in the library, over a percentage belonging to a
    // different one. (The dead name keeps being sent; the server defines that fallback, and
    // re-adding the section restores the viewer's pick.)
    await boxes()[1]!.setValue(false)
    expect((wrapper.vm as unknown as { selectedCard: string }).selectedCard).toBe('Lightning Bolt')

    // Re-adding it brings the pick back rather than having silently forgotten it.
    await boxes()[1]!.setValue(true)
    expect((wrapper.vm as unknown as { selectedCard: string }).selectedCard).toBe('Pyroblast')
  })
})
