import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Ref } from 'vue'
import { mount } from '@vue/test-utils'
import DeckStats from '@/components/decks/DeckStats.vue'
import type { DeckAnalytics, DeckSection } from '@/lib/api'

// The panel's numbers are the server's (issue #596), so what's left to test here is the
// two controls that choose *which* question is asked — which sections are the shuffled
// library, and which card the odds are for — plus that the odds slider reads the curve the
// response already carries instead of refetching per tick.
//
// The panel now rests collapsed, so every one of those controls is behind the disclosure:
// each case below opens it through the same button a viewer clicks (`expand`), and the
// resting state has cases of its own.

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

/** A curve with a real shape, not one filled bucket: the collapsed strip scales every bar
 * against the TALLEST one, so a stub whose buckets are all-or-nothing would let a broken
 * scale (sum, bucket count, a fixed denominator) pass unnoticed. The server always emits all
 * eight buckets including the zeroes (`compose`), and this mirrors that. */
const CURVE = [0, 12, 8, 4, 0, 0, 0, 0]

/** More than one colour, so "the pips carry weights, not just which colours" is testable —
 * and `C`, which the server emits like any other non-zero bucket. */
const COLOURS = [
  { key: 'R', label: 'Red', count: 40, color: '#ef4444' },
  { key: 'G', label: 'Green', count: 25, color: '#22c55e' },
  { key: 'C', label: 'Colorless', count: 10, color: '#a1a1aa' },
]

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
    land_copies: stub.landsOnly ? copies : 30,
    average_mana_value: copies > 0 && !stub.landsOnly ? 1 : null,
    mana_curve: CURVE.map((count, index) => ({
      key: String(index),
      label: index === 7 ? '7+' : String(index),
      count: stub.landsOnly ? 0 : count,
      color: null,
    })),
    colors: COLOURS,
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

/** Shape knobs for the stubbed composition, for the cases that need a differently built
 * deck than the default one (today: a deck with no nonland spells to curve). */
const stub = vi.hoisted(() => ({ landsOnly: false }))

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
  stub.landsOnly = false
})

const SECTIONS: DeckSection[] = [
  { id: 1, name: 'Mainboard', position: 0, is_maybeboard: false },
  { id: 2, name: 'Sideboard', position: 1, is_maybeboard: false },
  { id: 3, name: 'Tiny', position: 2, is_maybeboard: false },
]

function mountPanel() {
  return mount(DeckStats, { props: { game: 'mtg', deckId: 7, sections: SECTIONS } })
}

/** The panel rests collapsed, so a case about the controls opens it the way a viewer does —
 * through the disclosure, not by reaching into the component's state.
 *
 * Scoped to the header rather than matched on `[aria-expanded]`: the body it opens contains
 * the card select, which is a combobox and carries that attribute too. */
function disclosure(wrapper: ReturnType<typeof mountPanel>) {
  return wrapper.get<HTMLButtonElement>('[data-slot="card-header"] button')
}
async function expand(wrapper: ReturnType<typeof mountPanel>) {
  await disclosure(wrapper).trigger('click')
}

describe('DeckStats resting state', () => {
  it('rests collapsed, with none of the panel mounted', () => {
    const wrapper = mountPanel()

    // Not merely hidden: the heavy half isn't in the DOM at all until it's asked for.
    expect(disclosure(wrapper).attributes('aria-expanded')).toBe('false')
    expect(wrapper.find('input[type="checkbox"]').exists()).toBe(false)
    expect(wrapper.find('input[type="range"]').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('Draw probability')
    expect(wrapper.text()).not.toContain('Card types')
  })

  it('says what the deck is without opening it', () => {
    const wrapper = mountPanel()
    const text = wrapper.text()

    // The land ratio prints its own denominator: `total_copies` is the deck proper, which
    // counts a sideboard and a command zone, so a bare "40%" would be read against the 60 or
    // 99 the player has in mind.
    expect(text).toContain('Lands')
    expect(text).toContain('30')
    expect(text).toContain('40% of 75')
    expect(text).toContain('Avg mana value')
    expect(text).toContain('1.00')
    // The colour weights — one row per colour the deck plays, each carrying HOW MUCH. The
    // pips are decorative (`aria-hidden`), so the sr-only label is what names the colour.
    expect(wrapper.findAll('ul li').map((item) => item.text().replace(/\s+/g, ' '))).toEqual([
      'Red:40',
      'Green:25',
      'Colorless:10',
    ])
    // The curve strip — every bucket the server sent, zeroes included.
    expect(text).toContain('Mana curve (nonlands)')
    expect(wrapper.findAll('[role="img"][aria-label*="copies"]')).toHaveLength(8)
  })

  it('scales the strip against its tallest bucket, and fills only the ones with copies', () => {
    const wrapper = mountPanel()
    const tracks = wrapper.findAll('[role="img"][aria-label*="copies"]')
    const heightOf = (index: number) =>
      Number(/height:\s*([\d.]+)%/.exec(tracks[index]!.find('div').attributes('style') ?? '')?.[1])

    // CURVE is [0, 12, 8, 4, 0…]: the 12 is the full height and the rest are its fractions.
    // Scaling by anything else — the sum, the bucket count, a fixed denominator — leaves the
    // strip a row of slivers, which is the whole feature gone.
    expect(heightOf(1)).toBe(100)
    expect(heightOf(2)).toBeCloseTo(66.67, 1)
    expect(heightOf(3)).toBeCloseTo(33.33, 1)
    expect(heightOf(0)).toBe(0)

    // An empty bucket keeps its track but paints nothing, so a zero can't read as "one or
    // two"; a bucket with copies is never thinner than the floor.
    expect(tracks[0]!.find('div').classes()).not.toContain('min-h-0.5')
    expect(tracks[3]!.find('div').classes()).toContain('min-h-0.5')
  })

  it('words a curve of nothing rather than drawing eight empty tracks', () => {
    stub.landsOnly = true
    const wrapper = mountPanel()

    expect(wrapper.text()).toContain('No nonland spells yet.')
    expect(wrapper.find('[role="img"][aria-label*="copies"]').exists()).toBe(false)
    // Nothing to average is "—", not a zero the deck never claimed.
    expect(wrapper.text()).toContain('—')
  })

  it('keeps the viewer own draw question when the panel closes again', async () => {
    const wrapper = mountPanel()
    // At rest the server's default pick is whatever the deck holds most of — a basic land on
    // most decks — so the panel says nothing about draw odds until asked.
    expect(wrapper.text()).not.toContain('to see one in')

    await expand(wrapper)
    ;(wrapper.vm as unknown as { selectedCard: string }).selectedCard = 'Lightning Bolt'
    await wrapper.vm.$nextTick()
    await expand(wrapper)

    expect(disclosure(wrapper).attributes('aria-expanded')).toBe('false')
    // The pool is named, because the checkboxes that chose it are behind the disclosure.
    expect(wrapper.text()).toContain('Lightning Bolt')
    expect(wrapper.text()).toContain('to see one in 7 cards from a')
    expect(wrapper.text()).toContain('60')
  })

  it('opens and closes the panel without asking the server anything new', async () => {
    const wrapper = mountPanel()
    const before = { ...captured.params!.value }

    await expand(wrapper)
    expect(disclosure(wrapper).attributes('aria-expanded')).toBe('true')
    expect(wrapper.find('input[type="range"]').exists()).toBe(true)
    // The whole response is already here — a disclosure is not a question.
    expect(captured.params!.value).toEqual(before)

    // The body it controls is only addressable while it is showing.
    const detailsId = disclosure(wrapper).attributes('aria-controls')
    expect(detailsId).toBeTruthy()
    expect(wrapper.find(`#${detailsId}`).exists()).toBe(true)

    await expand(wrapper)
    expect(disclosure(wrapper).attributes('aria-controls')).toBeUndefined()
  })

  it('names itself, so the page two disclosures are told apart', () => {
    // `DeckBracket` puts a second button reading "Details" a few rows up this page.
    const label = disclosure(mountPanel()).attributes('aria-label')
    expect(label).toBe('Details for deck analytics')
  })
})

describe('DeckStats draw sections', () => {
  it('starts on the library the server picked and lets the viewer widen it', async () => {
    const wrapper = mountPanel()
    await expand(wrapper)

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
    await expand(wrapper)

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
    await expand(wrapper)
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
    await expand(wrapper)

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

  it('renders the composition the server folded', async () => {
    const wrapper = mountPanel()
    await expand(wrapper)
    const text = wrapper.text()
    expect(text).toContain('75')
    expect(text).toContain('Red')
    expect(text).toContain('Instant')
  })
})

describe('DeckStats public mode', () => {
  it('renders from the handle-addressed mirror when given a handle', async () => {
    captured.params = null
    const wrapper = mount(DeckStats, {
      props: { game: 'mtg', deckId: 7, sections: SECTIONS, handle: 'alice-0001' },
    })
    // The public hook was selected at mount, so the authed one never ran.
    expect(captured.params).toBeNull()
    // A shared deck gets the same resting summary and the same way into the rest of it.
    expect(wrapper.text()).toContain('40% of 75')
    await expand(wrapper)
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

  it('keeps the disclosure in place while the first numbers are in flight', () => {
    // The control is present but dead, so the header's right edge doesn't shift the moment
    // the response lands — the whole point of drawing the resting shape.
    query.pending = true
    const wrapper = mountPanel()
    expect(disclosure(wrapper).element.disabled).toBe(true)
  })

  it('says so when the numbers cannot be worked out', () => {
    query.failed = true
    const wrapper = mountPanel()
    expect(wrapper.text()).toContain("couldn't be worked out")
    expect(wrapper.find('input[type="range"]').exists()).toBe(false)
    // Nothing to disclose, so nothing offers to.
    expect(wrapper.find('[data-slot="card-header"] button').exists()).toBe(false)
  })

  it('marks the previous answer as being replaced while a new one is in flight', async () => {
    query.fetching = true
    const wrapper = mountPanel()

    // keepPreviousData deliberately leaves the last answer on screen, so the cue is the
    // only thing distinguishing "your click did nothing" from "your click is in the air".
    expect(wrapper.text()).toContain('Recalculating…')
    expect(wrapper.find('[data-slot="card"]').attributes('aria-busy')).toBe('true')
    await expand(wrapper)
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
    await expand(wrapper)
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
    await expand(wrapper)
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
