import { beforeEach, describe, expect, it } from 'vitest'
import type { Card, DeckCardEntry, DeckSection } from '@/lib/api'
import { cardCopyLimit, deckZone, evaluateDeckRules, hasDeckRules } from '@/lib/deckRules'
import { makeCard } from '@/test/fixtures'

const COMMAND = 1
const MAIN = 2
const SIDE = 3
const SECTIONS: DeckSection[] = [
  { id: COMMAND, name: 'Commander', position: 0, is_maybeboard: false },
  { id: MAIN, name: 'Creatures', position: 1, is_maybeboard: false },
  { id: SIDE, name: 'Sideboard', position: 2, is_maybeboard: false },
]

let serial = 0
beforeEach(() => {
  serial = 0
})

/** A card with a plain creature type line — the fixture's default is a basic land, which
 * every copy-limit rule deliberately ignores. */
function card(name: string, over: Partial<Card> = {}): Card {
  serial += 1
  return makeCard(`${name}-${serial}`, {
    name,
    type_line: 'Creature — Human',
    color_identity: [],
    ...over,
  })
}

function entry(sectionId: number, of: Card, copies = 1): DeckCardEntry {
  return { card: of, section_id: sectionId, quantity: copies, foil_quantity: 0 }
}

/** `count` distinct one-of cards, so a deck reaches a size without tripping copy limits. */
function filler(count: number, sectionId = MAIN): DeckCardEntry[] {
  return Array.from({ length: count }, (_, index) => entry(sectionId, card(`Filler ${index}`)))
}

const LEGEND = (name: string, over: Partial<Card> = {}) =>
  card(name, { type_line: 'Legendary Creature — Human Wizard', ...over })

function rules(format: string, entries: DeckCardEntry[], sections = SECTIONS) {
  return evaluateDeckRules(format, entries, sections)
}

function messages(format: string, entries: DeckCardEntry[], sections = SECTIONS): string[] {
  return rules(format, entries, sections).violations.map((violation) => violation.message)
}

describe('deckZone', () => {
  it('reads the zone off the section name, case- and spelling-tolerantly', () => {
    expect(deckZone('Commander')).toBe('command')
    expect(deckZone('  commanders ')).toBe('command')
    expect(deckZone('Command Zone')).toBe('command')
    expect(deckZone('Signature Spell')).toBe('command')
    expect(deckZone('Sideboard')).toBe('sideboard')
    expect(deckZone('Companion')).toBe('sideboard')
    expect(deckZone('Creatures')).toBe('main')
    expect(deckZone('Ramp')).toBe('main')
  })
})

describe('cardCopyLimit', () => {
  it('leaves an ordinary card to its format limit', () => {
    expect(cardCopyLimit(card('Lightning Bolt', { type_line: 'Instant' }))).toBeNull()
  })

  it('lets basic lands (snow included) run unbounded', () => {
    expect(cardCopyLimit(card('Island', { type_line: 'Basic Land — Island' }))).toBe(Infinity)
    expect(
      cardCopyLimit(card('Snow-Covered Swamp', { type_line: 'Basic Snow Land — Swamp' })),
    ).toBe(Infinity)
    expect(cardCopyLimit(card('Dryad Arbor', { type_line: 'Land Creature — Forest Dryad' }))).toBe(
      null,
    )
  })

  it('reads "any number" and "up to <n>" straight out of the oracle text', () => {
    expect(
      cardCopyLimit(
        card('Rat Colony', {
          oracle_text: 'A deck can have any number of cards named Rat Colony.',
        }),
      ),
    ).toBe(Infinity)
    expect(
      cardCopyLimit(
        card('Seven Dwarves', {
          oracle_text: 'A deck can have up to seven cards named Seven Dwarves.',
        }),
      ),
    ).toBe(7)
    expect(
      cardCopyLimit(
        card('Nazgûl', { oracle_text: 'A deck can have up to nine cards named Nazgûl.' }),
      ),
    ).toBe(9)
  })
})

describe('multi-faced cards', () => {
  const face = (over: Partial<Card['faces'][number]> = {}) => ({
    name: null,
    mana_cost: null,
    type_line: null,
    oracle_text: null,
    power: null,
    toughness: null,
    loyalty: null,
    ...over,
  })

  it('reads type line and rules text off the faces when the top level has none', () => {
    // Scryfall leaves `oracle_text` null on a multi-faced card and puts it on the faces.
    const petitioners = card('Persistent Petitioners', {
      oracle_text: null,
      faces: [face({ oracle_text: 'A deck can have any number of cards named X.' })],
    })
    expect(cardCopyLimit(petitioners)).toBe(Infinity)

    const partnered = (name: string) =>
      card(name, {
        type_line: null,
        oracle_text: null,
        faces: [face({ type_line: 'Legendary Creature — Human', oracle_text: 'Partner' })],
      })
    expect(
      messages('commander', [
        entry(COMMAND, partnered('Alpha')),
        entry(COMMAND, partnered('Bravo')),
        ...filler(98),
      ]),
    ).toEqual([])
  })
})

describe('hasDeckRules', () => {
  it('covers the curated formats and nothing else', () => {
    expect(hasDeckRules('commander')).toBe(true)
    expect(hasDeckRules('modern')).toBe(true)
    expect(hasDeckRules('oathbreaker')).toBe(true)
    expect(hasDeckRules('gladiator')).toBe(true)
    expect(hasDeckRules('cube')).toBe(false)
  })

  it('says nothing at all about a format it has no profile for', () => {
    expect(rules('cube', filler(3))).toEqual({ violations: [], cardIssues: [] })
  })
})

describe('deck size', () => {
  it('stays quiet on an empty deck', () => {
    expect(rules('commander', [])).toEqual({ violations: [], cardIssues: [] })
  })

  it('warns while a Commander deck is still short of 100', () => {
    const [violation] = rules('commander', [
      entry(COMMAND, LEGEND('Atraxa')),
      ...filler(62),
    ]).violations
    expect(violation).toMatchObject({ rule: 'deck-size', severity: 'warning' })
    expect(violation!.message).toBe('63 of 100 cards — 37 to go.')
  })

  it('calls an over-100 Commander deck illegal, counting the commander and foils', () => {
    const over = rules('commander', [
      entry(COMMAND, LEGEND('Atraxa')),
      ...filler(99),
      { card: card('Extra Foil'), section_id: MAIN, quantity: 0, foil_quantity: 3 },
    ]).violations
    expect(over[0]).toMatchObject({ rule: 'deck-size', severity: 'error' })
    expect(over[0]!.message).toBe('103 cards — 3 over the 100-card limit.')
  })

  it('accepts an exactly-100 Commander deck', () => {
    const exact = rules('commander', [entry(COMMAND, LEGEND('Atraxa')), ...filler(99)])
    expect(exact.violations).toEqual([])
    expect(exact.cardIssues).toEqual([])
  })

  it('treats 60 as a floor in constructed, not a ceiling', () => {
    expect(messages('modern', filler(58))).toEqual(['58 of 60 cards — 2 to go.'])
    expect(messages('modern', filler(75))).toEqual([])
  })

  it('leaves the sideboard out of the deck size but caps it at fifteen', () => {
    expect(messages('modern', [...filler(60), ...filler(15, SIDE)])).toEqual([])
    expect(messages('modern', [...filler(60), ...filler(16, SIDE)])).toEqual([
      '16 cards in the sideboard — the limit is 15.',
    ])
  })

  it('has no sideboard rule in Commander, and keeps a companion out of the 100', () => {
    const sections = [...SECTIONS, { id: 9, name: 'Companion', position: 3, is_maybeboard: false }]
    const entries = [entry(COMMAND, LEGEND('Atraxa')), ...filler(99), entry(9, LEGEND('Lurrus'))]
    expect(messages('commander', entries, sections)).toEqual([])
  })

  it('counts a stray Commander section as deck cards in a format without one', () => {
    expect(messages('modern', [...filler(59), entry(COMMAND, card('Misfiled'))])).toEqual([])
  })
})

describe('copy limit', () => {
  it('flags a fifth copy in constructed and every printing of it', () => {
    const bolt = card('Lightning Bolt', { type_line: 'Instant' })
    const otherArt = card('Lightning Bolt', { type_line: 'Instant' })
    const result = rules('modern', [...filler(56), entry(MAIN, bolt, 3), entry(SIDE, otherArt, 2)])
    expect(result.cardIssues).toEqual([
      { cardId: bolt.id, name: 'Lightning Bolt', status: 'over_limit', quantity: 5 },
      { cardId: otherArt.id, name: 'Lightning Bolt', status: 'over_limit', quantity: 5 },
    ])
  })

  it('allows exactly four across the deck and sideboard', () => {
    const bolt = card('Lightning Bolt', { type_line: 'Instant' })
    expect(
      rules('modern', [...filler(57), entry(MAIN, bolt, 3), entry(SIDE, bolt, 1)]).cardIssues,
    ).toEqual([])
  })

  it('is singleton in Commander, basics excepted', () => {
    const ring = card('Sol Ring', { type_line: 'Artifact' })
    const island = card('Island', { type_line: 'Basic Land — Island' })
    const result = rules('commander', [
      entry(COMMAND, LEGEND('Atraxa')),
      entry(MAIN, ring, 2),
      entry(MAIN, island, 30),
      ...filler(67),
    ])
    expect(result.cardIssues).toEqual([
      { cardId: ring.id, name: 'Sol Ring', status: 'over_limit', quantity: 2 },
    ])
  })

  it("honours a card's own text over the format's singleton rule", () => {
    const rats = card('Relentless Rats', {
      type_line: 'Creature — Rat',
      oracle_text: 'A deck can have any number of cards named Relentless Rats.',
    })
    const dwarves = card('Seven Dwarves', {
      type_line: 'Creature — Dwarf',
      oracle_text: 'A deck can have up to seven cards named Seven Dwarves.',
    })
    const result = rules('commander', [
      entry(COMMAND, LEGEND('Marrow-Gnawer')),
      entry(MAIN, rats, 40),
      entry(MAIN, dwarves, 8),
      ...filler(51),
    ])
    expect(result.cardIssues).toEqual([
      { cardId: dwarves.id, name: 'Seven Dwarves', status: 'over_limit', quantity: 8 },
    ])
  })
})

describe('command zone', () => {
  const deck = (commanders: DeckCardEntry[], rest = 0) => [...commanders, ...filler(rest)]

  it('warns rather than fails when no commander has been chosen', () => {
    const [violation] = rules('commander', filler(40)).violations.filter(
      (item) => item.rule === 'command-zone',
    )
    expect(violation).toMatchObject({ severity: 'warning' })
    expect(violation!.message).toBe('No commander — put one in a section named "Commander".')
  })

  it('rejects a commander that cannot lead a deck', () => {
    const ring = card('Sol Ring', { type_line: 'Artifact' })
    expect(messages('commander', deck([entry(COMMAND, ring)], 99))).toEqual([
      "Sol Ring can't be your commander — a commander must be a legendary creature (or a card that says it can be your commander).",
    ])
  })

  it('accepts a planeswalker that says it can be your commander, but not a plain one', () => {
    const teferi = card('Teferi, Temporal Archmage', {
      type_line: 'Legendary Planeswalker — Teferi',
      oracle_text: 'Teferi, Temporal Archmage can be your commander.',
    })
    expect(messages('commander', deck([entry(COMMAND, teferi)], 99))).toEqual([])

    const jace = card('Jace, the Mind Sculptor', { type_line: 'Legendary Planeswalker — Jace' })
    expect(messages('commander', deck([entry(COMMAND, jace)], 99))[0]).toContain("can't be your")
  })

  it('lets Brawl lead with any legendary planeswalker', () => {
    const jace = card('Jace, the Mind Sculptor', { type_line: 'Legendary Planeswalker — Jace' })
    expect(messages('brawl', deck([entry(COMMAND, jace)], 99))).toEqual([])
    expect(messages('standardbrawl', deck([entry(COMMAND, jace)], 59))).toEqual([])
  })

  it('rejects a third commander outright', () => {
    const three = deck(
      [
        entry(COMMAND, LEGEND('Alpha')),
        entry(COMMAND, LEGEND('Bravo')),
        entry(COMMAND, LEGEND('Charlie')),
      ],
      97,
    )
    expect(messages('commander', three)).toEqual([
      '3 cards in the command zone — a deck has one commander, or two that pair.',
    ])
  })

  it('rejects two commanders that do not pair', () => {
    const pair = deck([entry(COMMAND, LEGEND('Alpha')), entry(COMMAND, LEGEND('Bravo'))], 98)
    expect(messages('commander', pair)).toEqual([
      "Alpha and Bravo can't be commanders together — a pair needs Partner, Friends forever, Doctor's companion, or Choose a Background.",
    ])
  })

  it('counts a second copy of one commander as two commanders', () => {
    const solo = LEGEND('Alpha')
    expect(messages('commander', deck([entry(COMMAND, solo, 2)], 98))[0]).toContain(
      "can't be commanders together",
    )
  })

  it('accepts every sanctioned pairing', () => {
    const partner = (name: string) =>
      LEGEND(name, {
        oracle_text: 'Partner (You can have two commanders if both have partner.)',
      })
    expect(
      messages(
        'commander',
        deck([entry(COMMAND, partner('Tana')), entry(COMMAND, partner('Tymna'))], 98),
      ),
    ).toEqual([])

    const forever = (name: string) => LEGEND(name, { oracle_text: 'Friends forever' })
    expect(
      messages(
        'commander',
        deck([entry(COMMAND, forever('Amy')), entry(COMMAND, forever('Rory'))], 98),
      ),
    ).toEqual([])

    const doctor = LEGEND('The Fourth Doctor', {
      type_line: 'Legendary Creature — Time Lord Doctor',
    })
    const companion = LEGEND('Sarah Jane Smith', { oracle_text: "Doctor's companion" })
    expect(
      messages('commander', deck([entry(COMMAND, doctor), entry(COMMAND, companion)], 98)),
    ).toEqual([])

    const chooser = LEGEND('Wilson', { oracle_text: 'Choose a Background' })
    const background = card('Criminal Past', { type_line: 'Legendary Enchantment — Background' })
    expect(
      messages('commander', deck([entry(COMMAND, chooser), entry(COMMAND, background)], 98)),
    ).toEqual([])
  })

  it('pairs "Partner with" only with the card it names', () => {
    const pir = LEGEND('Pir, Imaginative Rascal', {
      oracle_text:
        'Partner with Toothy, Imaginary Friend (When this creature enters, target opponent may put Toothy into their hand.)',
    })
    const toothy = LEGEND('Toothy, Imaginary Friend', {
      oracle_text: 'Partner with Pir, Imaginative Rascal',
    })
    const stranger = LEGEND('Someone Else')
    expect(messages('commander', deck([entry(COMMAND, pir), entry(COMMAND, toothy)], 98))).toEqual(
      [],
    )
    expect(
      messages('commander', deck([entry(COMMAND, pir), entry(COMMAND, stranger)], 98))[0],
    ).toContain("can't be commanders together")
  })

  it('has no command zone in Gladiator, so a 100-card singleton deck is clean', () => {
    expect(messages('gladiator', filler(100))).toEqual([])
  })

  it('wants a planeswalker and a signature spell in Oathbreaker', () => {
    const walker = card('Kaya, Ghost Assassin', { type_line: 'Legendary Planeswalker — Kaya' })
    const spell = card('Anguished Unmaking', { type_line: 'Instant' })
    expect(
      messages('oathbreaker', [entry(COMMAND, walker), entry(COMMAND, spell), ...filler(58)]),
    ).toEqual([])

    const second = card('Teferi, Hero of Dominaria', {
      type_line: 'Legendary Planeswalker — Teferi',
    })
    expect(
      messages('oathbreaker', [entry(COMMAND, walker), entry(COMMAND, second), ...filler(58)])[0],
    ).toContain('exactly one legendary planeswalker and one instant or sorcery')

    expect(messages('oathbreaker', [entry(COMMAND, walker), ...filler(59)])[0]).toBe(
      'No oathbreaker and signature spell — put a legendary planeswalker and one instant or sorcery in a section named "Oathbreaker".',
    )
  })
})

describe('colour identity', () => {
  const atraxa = () => LEGEND('Atraxa, Praetors’ Voice', { color_identity: ['W', 'U', 'B', 'G'] })

  it('flags every card straying outside the command zone identity', () => {
    const bolt = card('Lightning Bolt', { type_line: 'Instant', color_identity: ['R'] })
    const path = card('Path to Exile', { type_line: 'Instant', color_identity: ['W'] })
    const result = rules('commander', [
      entry(COMMAND, atraxa()),
      entry(MAIN, bolt),
      entry(MAIN, path),
      ...filler(97),
    ])
    expect(result.cardIssues).toEqual([
      { cardId: bolt.id, name: 'Lightning Bolt', status: 'off_colour', quantity: 1 },
    ])
    expect(result.violations.map((violation) => violation.message)).toEqual([
      "1 card falls outside Atraxa, Praetors’ Voice's colour identity ({W}{U}{B}{G}).",
    ])
  })

  it('unions both commanders of a pair and pluralizes the count', () => {
    const partner = (name: string, colours: string[]) =>
      LEGEND(name, { color_identity: colours, oracle_text: 'Partner' })
    const bolt = card('Lightning Bolt', { type_line: 'Instant', color_identity: ['R'] })
    const swamp = card('Bog', { type_line: 'Land', color_identity: ['B'] })
    const result = rules('commander', [
      entry(COMMAND, partner('Tana', ['R', 'G'])),
      entry(COMMAND, partner('Tymna', ['W', 'B'])),
      ...filler(96),
      entry(MAIN, bolt),
      entry(MAIN, swamp),
    ])
    expect(result.cardIssues).toEqual([])
    expect(result.violations).toEqual([])
  })

  it('reports a colourless commander as colourless', () => {
    const kozilek = LEGEND('Kozilek', { color_identity: [] })
    const bolt = card('Lightning Bolt', { type_line: 'Instant', color_identity: ['R'] })
    const swamp = card('Bog', { type_line: 'Land', color_identity: ['B'] })
    const result = rules('commander', [
      entry(COMMAND, kozilek),
      entry(MAIN, bolt),
      entry(MAIN, swamp),
      ...filler(97),
    ])
    expect(result.cardIssues.map((issue) => issue.name)).toEqual(['Lightning Bolt', 'Bog'])
    expect(result.violations[0]!.message).toBe(
      "2 cards fall outside Kozilek's colour identity (colourless).",
    )
  })

  it('never judges colour identity while the command zone is empty', () => {
    const bolt = card('Lightning Bolt', { type_line: 'Instant', color_identity: ['R'] })
    const result = rules('commander', [entry(MAIN, bolt), ...filler(99)])
    expect(result.cardIssues).toEqual([])
    expect(result.violations.every((violation) => violation.rule !== 'colour-identity')).toBe(true)
  })

  it('leaves an off-colour card alone in a format without a command zone', () => {
    const bolt = card('Lightning Bolt', { type_line: 'Instant', color_identity: ['R'] })
    expect(rules('modern', [entry(MAIN, bolt, 4), ...filler(56)]).cardIssues).toEqual([])
  })
})
