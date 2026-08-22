import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import type { Deck } from '@/lib/api'
import DeckIdentity from '../DeckIdentity.vue'

/** A built, colourless, commanderless deck — each case overrides only what it asserts on. */
function makeDeck(over: Partial<Deck> = {}): Deck {
  return {
    id: 1,
    game: 'mtg',
    name: 'Test Deck',
    description: null,
    format: null,
    folder_id: null,
    is_public: false,
    card_count: 60,
    color_identity: [],
    commanders: [],
    value_usd: null,
    created_at: '',
    updated_at: '',
    ...over,
  }
}

type Wrapper = ReturnType<typeof mount>

/** The mana-font glyph classes rendered, in order (`ms-w`, `ms-c`, …). */
function pips(wrapper: Wrapper): string[] {
  return wrapper.findAll('i.ms').map((pip) => pip.classes().find((c) => /^ms-[a-z0-9]+$/.test(c))!)
}

/** The commander label — selected by its own class, since `ManaSymbols` contributes the
 *  first `<span>` in the row. */
function label(wrapper: Wrapper) {
  return wrapper.find('span.truncate')
}

describe('DeckIdentity', () => {
  it('renders the colour identity as pips, in the order the API sent them', () => {
    const wrapper = mount(DeckIdentity, {
      props: { deck: makeDeck({ color_identity: ['W', 'U', 'B'] }) },
    })

    expect(pips(wrapper)).toEqual(['ms-w', 'ms-u', 'ms-b'])
  })

  it('shows a colourless pip for a deck that plays no colour', () => {
    const wrapper = mount(DeckIdentity, {
      props: { deck: makeDeck({ color_identity: [], card_count: 99 }) },
    })

    expect(pips(wrapper)).toEqual(['ms-c'])
  })

  it('renders nothing when there is no colour to read — an unbuilt deck is not colourless', () => {
    const wrapper = mount(DeckIdentity, { props: { deck: makeDeck({ color_identity: null }) } })

    expect(wrapper.find('p').exists()).toBe(false)
  })

  it('trusts the API over the card count for that call', () => {
    // A sideboard-only deck: cards counted, but nothing that colours the deck. Inferring
    // from `card_count` here would claim the deck is colourless.
    const wrapper = mount(DeckIdentity, {
      props: { deck: makeDeck({ color_identity: null, card_count: 15 }) },
    })

    expect(wrapper.find('p').exists()).toBe(false)
  })

  it('names the commander beside its colours', () => {
    const wrapper = mount(DeckIdentity, {
      props: {
        deck: makeDeck({
          color_identity: ['W', 'U', 'B', 'R', 'G'],
          commanders: [{ card_id: 'abc', name: "Atraxa, Praetors' Voice" }],
        }),
      },
    })

    expect(label(wrapper).text()).toContain("Atraxa, Praetors' Voice")
    expect(pips(wrapper)).toHaveLength(5)
  })

  it('joins a partner pair, and leaves a crowded zone to truncation rather than a count', () => {
    const pair = mount(DeckIdentity, {
      props: {
        deck: makeDeck({
          commanders: [
            { card_id: 'a', name: 'Tana, the Bloodsower' },
            { card_id: 'b', name: 'Tymna the Weaver' },
          ],
        }),
      },
    })
    expect(label(pair).text()).toContain('Tana, the Bloodsower & Tymna the Weaver')

    // The API caps how many it sends, so a "+N more" here could only ever be a floor —
    // every name received is shown, the CSS truncates, and `title` carries the whole thing.
    const crowded = mount(DeckIdentity, {
      props: {
        deck: makeDeck({
          commanders: [
            { card_id: 'a', name: 'One' },
            { card_id: 'b', name: 'Two' },
            { card_id: 'c', name: 'Three' },
          ],
        }),
      },
    })
    expect(label(crowded).attributes('title')).toBe('One & Two & Three')
    expect(label(crowded).classes()).toContain('truncate')
  })

  it('frames both halves for a screen reader instead of reading bare mana fragments', () => {
    const wrapper = mount(DeckIdentity, {
      props: {
        deck: makeDeck({
          color_identity: ['W', 'U'],
          commanders: [{ card_id: 'a', name: 'Tymna the Weaver' }],
        }),
      },
    })

    // One atomic image for the pip row, so it reads as an identity and not "White mana,
    // Blue mana" with no context.
    const identity = wrapper.find('[role="img"]')
    expect(identity.attributes('aria-label')).toBe('Colour identity: white, blue')
    expect(label(wrapper).find('.sr-only').text()).toBe('Commander:')
  })

  it('calls a colourless deck colourless in its label too', () => {
    const wrapper = mount(DeckIdentity, { props: { deck: makeDeck({ color_identity: [] }) } })

    expect(wrapper.find('[role="img"]').attributes('aria-label')).toBe(
      'Colour identity: colourless',
    )
  })

  it('renders on a named commander even if there is no colour to read', () => {
    const wrapper = mount(DeckIdentity, {
      props: {
        deck: makeDeck({
          // The API can't produce this pair, but "blank" means nothing to say — a header
          // that named a commander would still have something.
          color_identity: null,
          card_count: 0,
          commanders: [{ card_id: 'a', name: 'Krenko, Mob Boss' }],
        }),
      },
    })

    expect(label(wrapper).text()).toContain('Krenko, Mob Boss')
    expect(pips(wrapper)).toEqual([])
  })
})
