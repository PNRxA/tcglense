// Deck-construction rules per format (issue #557 follow-up). The per-card side of legality
// — Scryfall's banned/restricted data — lives in `lib/legality.ts`; this module owns the
// checks that judge the deck as a *whole*: how many cards it holds, how many copies of one
// name, what the command zone contains, and whether the 99 stay inside the commander's
// colour identity. `legality.ts` composes the two into the single `DeckLegality` the deck
// views render, and nothing here imports from there, so the dependency stays one-way.
//
// Everything is derived from data already on the `Card` payload — type line, colour
// identity, oracle text — so a newly printed Partner commander or "any number of cards
// named" card obeys the rules the day the catalog ingests it, with no curated list to
// maintain. Same philosophy as the per-card check: a false "in breach" is worse than a
// miss, so an unrecognised format, an empty deck, or a card we can't read confidently is
// skipped rather than guessed at.

import type { Card, DeckCardEntry, DeckSection } from '@/lib/api'

// ---------- Zones ----------

/**
 * Which zone a section's cards sit in. Derived from the section *name* (a deck has no
 * per-section zone column, and users file cards by category), so this is deliberately
 * generous: anything unrecognised is the deck proper.
 */
export type DeckZone = 'command' | 'sideboard' | 'main'

/** Section names that mean "command zone" — the deck's seeded `Commander` plus the
 * spellings imports and Oathbreaker players arrive with. */
const COMMAND_ZONE_NAMES = new Set([
  'commander',
  'commanders',
  'command zone',
  'oathbreaker',
  'oathbreakers',
  'signature spell',
  'signature spells',
])

/** Section names that sit beside the deck rather than in it. A companion lives in the
 * sideboard in constructed and outside the 100 in Commander, so it counts as one here. */
const SIDEBOARD_NAMES = new Set([
  'sideboard',
  'sideboards',
  'side board',
  'companion',
  'companions',
])

/** The zone a section name puts its cards in. */
export function deckZone(name: string): DeckZone {
  const key = name.trim().toLowerCase()
  if (COMMAND_ZONE_NAMES.has(key)) return 'command'
  if (SIDEBOARD_NAMES.has(key)) return 'sideboard'
  return 'main'
}

/** Ids of the sections holding the command zone — `legality.ts` needs them to tell a
 * Pauper Commander's uncommon commander from the same card sitting in the 99. */
export function commandZoneSectionIds(sections: DeckSection[]): ReadonlySet<number> {
  return new Set(
    sections.filter((section) => deckZone(section.name) === 'command').map((section) => section.id),
  )
}

// ---------- Card predicates ----------

/** The front face's type line, lowercased — the only face that decides a card's role. */
function frontTypeLine(card: Card): string {
  const line = card.type_line ?? card.faces[0]?.type_line ?? ''
  return line.split('//')[0]!.toLowerCase()
}

function hasType(card: Card, ...words: string[]): boolean {
  const line = frontTypeLine(card)
  return words.every((word) => new RegExp(`\\b${word}\\b`).test(line))
}

function isBasicLand(card: Card): boolean {
  return hasType(card, 'basic', 'land')
}

/**
 * A card's rules text. Scryfall leaves the top-level `oracle_text` null on a multi-faced
 * card and puts the text on the faces, so fall back to those — otherwise a transforming
 * commander would read as having no abilities at all.
 */
function oracleText(card: Card): string {
  return card.oracle_text ?? card.faces.map((face) => face.oracle_text ?? '').join('\n')
}

/**
 * Oracle text as lowercased ability lines with reminder text stripped, so a keyword test
 * matches the ability itself and never the parenthetical that explains it (every Partner
 * card's reminder text names Partner, and so does nothing else).
 */
function abilityLines(card: Card): string[] {
  return oracleText(card)
    .replace(/\([^)]*\)/g, '')
    .replace(/[‘’]/g, "'")
    .split('\n')
    .map((line) => line.trim().toLowerCase())
    .filter(Boolean)
}

/** Whether the card has a keyword ability on a line of its own (with or without a cost
 * or subject clause after it — "Partner with …", "Partner—Survivors"). */
function hasAbility(card: Card, keyword: string): boolean {
  return abilityLines(card).some(
    (line) => line === keyword || new RegExp(`^${keyword}[ \\u2014-]`).test(line),
  )
}

/** The card named by a "Partner with <name>" ability, lowercased, or null. */
function partnerWithName(card: Card): string | null {
  for (const line of abilityLines(card)) {
    const match = /^partner with (.+?)\.?$/.exec(line)
    if (match) return match[1]!.trim()
  }
  return null
}

const NUMBER_WORDS: Record<string, number> = {
  one: 1,
  two: 2,
  three: 3,
  four: 4,
  five: 5,
  six: 6,
  seven: 7,
  eight: 8,
  nine: 9,
  ten: 10,
  eleven: 11,
  twelve: 12,
}

/**
 * How many copies of this card a deck may hold *regardless* of its format, or `null` when
 * the format's own limit applies. Basic lands are unbounded, and so is the "A deck can have
 * any number of cards named …" cycle (Relentless Rats, Rat Colony, Shadowborn Apostle,
 * Persistent Petitioners, Dragon's Approach, Slime Against Humanity, …) — including in
 * singleton formats, where rule 903.5b lets that text override the one-copy rule. Seven
 * Dwarves and Nazgûl name their own cap ("up to seven/nine"), which is read from the same
 * sentence rather than hard-coded, so a future card in either cycle needs no code change.
 */
export function cardCopyLimit(card: Card): number | null {
  if (isBasicLand(card)) return Number.POSITIVE_INFINITY
  const text = oracleText(card)
  if (/a deck can have any number of cards named/i.test(text)) return Number.POSITIVE_INFINITY
  const capped = /a deck can have up to (\w+) cards named/i.exec(text)
  return capped ? (NUMBER_WORDS[capped[1]!.toLowerCase()] ?? null) : null
}

// ---------- Format rule table ----------

/** How a format's command zone works. `noun` names its card in messages. */
interface CommandZoneRule {
  kind: 'commander' | 'brawl' | 'pdh' | 'oathbreaker'
  noun: string
  /** Whether Partner / Friends forever / a Background can make the zone a pair. */
  allowPairs: boolean
}

interface FormatRules {
  /** Cards in the deck proper, command zone included: an exact count or a floor. */
  size: { exact: number; min?: undefined } | { min: number; exact?: undefined }
  /** Copies of one card name across the deck, sideboard included (1 = singleton). */
  maxCopies: number
  /** Cards allowed in sideboard sections; omitted when the format has no sideboard. */
  maxSideboard?: number
  commandZone?: CommandZoneRule
}

const COMMANDER_ZONE: CommandZoneRule = { kind: 'commander', noun: 'commander', allowPairs: true }
// Brawl leads with a legendary creature *or* planeswalker. Arena's Brawl queues have
// followed paper on Partner/Background, so pairs are allowed here too — being permissive
// keeps a legal deck from being called illegal.
const BRAWL_ZONE: CommandZoneRule = { kind: 'brawl', noun: 'commander', allowPairs: true }
// Pauper Commander leads with an *uncommon creature* — legendary is not required, and most
// PDH commanders aren't. The rarity half of that rule needs no check here: Scryfall marks
// the eligible uncommons `restricted` in `paupercommander`, which `legality.ts` reads as
// "legal only as the commander".
const PDH_ZONE: CommandZoneRule = { kind: 'pdh', noun: 'commander', allowPairs: true }
const OATHBREAKER_ZONE: CommandZoneRule = {
  kind: 'oathbreaker',
  noun: 'oathbreaker',
  allowPairs: false,
}

const CONSTRUCTED: FormatRules = { size: { min: 60 }, maxCopies: 4, maxSideboard: 15 }
const EDH: FormatRules = { size: { exact: 100 }, maxCopies: 1, commandZone: COMMANDER_ZONE }

/**
 * Construction rules per legality key. A format absent from this table is evaluated on its
 * per-card legality alone — the deck-wide checks simply don't run, which is why an
 * unsupported format can never produce a wrong "illegal" verdict.
 */
const FORMAT_RULES: Record<string, FormatRules> = {
  standard: CONSTRUCTED,
  pioneer: CONSTRUCTED,
  modern: CONSTRUCTED,
  legacy: CONSTRUCTED,
  vintage: CONSTRUCTED,
  pauper: CONSTRUCTED,
  alchemy: CONSTRUCTED,
  historic: CONSTRUCTED,
  timeless: CONSTRUCTED,
  penny: CONSTRUCTED,
  premodern: CONSTRUCTED,
  oldschool: CONSTRUCTED,
  commander: EDH,
  duel: EDH,
  predh: EDH,
  paupercommander: { size: { exact: 100 }, maxCopies: 1, commandZone: PDH_ZONE },
  brawl: { size: { exact: 100 }, maxCopies: 1, commandZone: BRAWL_ZONE },
  // Both Arena Brawl queues that build off Standard are 60-card decks; the 100-card
  // variant is `brawl` above (Historic/Timeless Brawl).
  standardbrawl: { size: { exact: 60 }, maxCopies: 1, commandZone: BRAWL_ZONE },
  competitivebrawl: { size: { exact: 60 }, maxCopies: 1, commandZone: BRAWL_ZONE },
  // 100-card singleton highlander with no command zone.
  gladiator: { size: { exact: 100 }, maxCopies: 1 },
  // 58 cards + the oathbreaker planeswalker + its signature spell.
  oathbreaker: { size: { exact: 60 }, maxCopies: 1, commandZone: OATHBREAKER_ZONE },
}

/** Whether a format has deck-construction rules at all (the deck-size/singleton checks). */
export function hasDeckRules(formatKey: string): boolean {
  return formatKey in FORMAT_RULES
}

// ---------- Command-zone eligibility ----------

/** Whether a card may lead a deck in this kind of command zone. */
function canLead(card: Card, kind: CommandZoneRule['kind']): boolean {
  if (kind === 'oathbreaker') return hasType(card, 'legendary', 'planeswalker')
  if (kind === 'pdh') return hasType(card, 'creature')
  if (hasType(card, 'legendary', 'creature')) return true
  // "can be your commander" covers the designed-for-the-zone planeswalkers and oddities;
  // a Background is only ever a commander (paired with "Choose a Background").
  if (abilityLines(card).some((line) => line.includes('can be your commander'))) return true
  if (hasType(card, 'background')) return true
  // Rule 903.3a reads the card's characteristics *outside* the battlefield, so a legendary
  // card its own text turns into a creature everywhere but there (Grist, the Hunger Tide)
  // leads a deck even though its printed front face is a planeswalker.
  if (
    hasType(card, 'legendary') &&
    abilityLines(card).some(
      (line) => line.includes("isn't on the battlefield") && line.includes('creature'),
    )
  )
    return true
  return kind === 'brawl' && hasType(card, 'legendary', 'planeswalker')
}

/** Whether two cards may share a command zone (Partner and its cousins). */
function pairAllowed(left: Card, right: Card): boolean {
  const partnered = (card: Card) => hasAbility(card, 'partner') && !partnerWithName(card)
  if (partnered(left) && partnered(right)) return true
  const named = (from: Card, to: Card) => partnerWithName(from) === to.name.toLowerCase()
  if (named(left, right) || named(right, left)) return true
  if (hasAbility(left, 'friends forever') && hasAbility(right, 'friends forever')) return true
  const pairs = (ability: string, test: (card: Card) => boolean) =>
    (hasAbility(left, ability) && test(right)) || (hasAbility(right, ability) && test(left))
  if (pairs("doctor's companion", (card) => hasType(card, 'time lord doctor'))) return true
  if (pairs('choose a background', (card) => hasType(card, 'background'))) return true
  return false
}

// ---------- Evaluation ----------

/** A per-card breach the construction rules found (the deck views chip these on tiles). */
export type DeckRuleCardStatus = 'off_colour' | 'over_limit'

export interface DeckRuleCardIssue {
  cardId: string
  name: string
  status: DeckRuleCardStatus
  /** Total copies of that name in the deck. */
  quantity: number
}

export type DeckRuleId =
  | 'deck-size'
  | 'sideboard-size'
  | 'command-zone'
  | 'commander-eligibility'
  | 'colour-identity'

export interface DeckRuleViolation {
  rule: DeckRuleId
  /** `error` = illegal as it stands; `warning` = simply not finished being built yet. */
  severity: 'error' | 'warning'
  /** Ready-to-render sentence. */
  message: string
}

export interface DeckRuleResult {
  violations: DeckRuleViolation[]
  cardIssues: DeckRuleCardIssue[]
}

const EMPTY: DeckRuleResult = { violations: [], cardIssues: [] }

const COLOUR_ORDER = ['W', 'U', 'B', 'R', 'G']

/** Colour identity as its mana symbols in WUBRG order, or "colourless". */
function identityLabel(identity: Set<string>): string {
  if (identity.size === 0) return 'colourless'
  return COLOUR_ORDER.filter((colour) => identity.has(colour))
    .map((colour) => `{${colour}}`)
    .join('')
}

function joinNames(names: string[]): string {
  if (names.length <= 1) return names[0] ?? ''
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`
}

/** Copies of an entry, floored at zero (a deleted row can briefly read as negative). */
function copiesOf(entry: DeckCardEntry): number {
  return Math.max(0, entry.quantity + entry.foil_quantity)
}

/**
 * Judge a deck against its format's construction rules. `entries` must already be the deck
 * proper (maybeboard excluded, like everything else that answers "what is this deck"), and
 * `sections` supplies the names the zone split reads.
 *
 * Returns no violations at all when the format has no rule profile or the deck is empty —
 * there is nothing useful to say about either.
 */
export function evaluateDeckRules(
  formatKey: string,
  entries: DeckCardEntry[],
  sections: DeckSection[],
): DeckRuleResult {
  const rules = FORMAT_RULES[formatKey]
  if (!rules || entries.length === 0) return EMPTY

  const zoneById = new Map(sections.map((section) => [section.id, deckZone(section.name)]))
  const violations: DeckRuleViolation[] = []
  const cardIssues: DeckRuleCardIssue[] = []

  // Pass 1: split the deck into zones and fold copies by name (a name's total spans every
  // section and printing, so 3 of one art plus 2 of another is five copies).
  const commanders: Card[] = []
  const byName = new Map<string, { card: Card; ids: string[]; copies: number }>()
  let deckCopies = 0
  let sideboardCopies = 0
  for (const entry of entries) {
    const copies = copiesOf(entry)
    if (copies === 0) continue
    const zone = zoneById.get(entry.section_id) ?? 'main'
    const inCommandZone = zone === 'command' && rules.commandZone != null
    if (inCommandZone) for (let index = 0; index < copies; index += 1) commanders.push(entry.card)
    // A command-zone section in a format without one (a Modern deck still gets the seeded
    // `Commander` section) is just part of the deck.
    if (zone === 'sideboard') sideboardCopies += copies
    else deckCopies += copies

    const fold = byName.get(entry.card.name)
    if (fold) {
      fold.ids.push(entry.card.id)
      fold.copies += copies
    } else {
      byName.set(entry.card.name, { card: entry.card, ids: [entry.card.id], copies })
    }
  }

  // ---- Deck size ----
  const { exact, min } = rules.size
  const required = exact ?? min!
  if (deckCopies > 0 && deckCopies < required) {
    violations.push({
      rule: 'deck-size',
      severity: 'warning',
      message: `${deckCopies} of ${required} cards — ${required - deckCopies} to go.`,
    })
  } else if (exact != null && deckCopies > exact) {
    violations.push({
      rule: 'deck-size',
      severity: 'error',
      message: `${deckCopies} cards — ${deckCopies - exact} over the ${exact}-card limit.`,
    })
  }
  if (rules.maxSideboard != null && sideboardCopies > rules.maxSideboard) {
    violations.push({
      rule: 'sideboard-size',
      severity: 'error',
      message: `${sideboardCopies} cards in the sideboard — the limit is ${rules.maxSideboard}.`,
    })
  }

  // ---- Copy limit ----
  for (const { card, ids, copies } of byName.values()) {
    const limit = cardCopyLimit(card) ?? rules.maxCopies
    if (copies <= limit) continue
    for (const id of ids)
      cardIssues.push({ cardId: id, name: card.name, status: 'over_limit', quantity: copies })
  }

  // ---- Command zone ----
  const zone = rules.commandZone
  if (zone) {
    violations.push(...commandZoneViolations(zone, commanders))
    // Colour identity: the 99 may not stray outside the command zone's combined identity.
    // Skipped while the zone is empty — an unbuilt deck isn't off-colour.
    const identity = new Set(commanders.flatMap((card) => card.color_identity))
    if (commanders.length > 0) {
      // By name, not by row: the commander's own identity defines the deck's, and a second
      // printing of it in the 99 is a copy-limit matter, not an off-colour one.
      const commanderNames = new Set(commanders.map((card) => card.name))
      const offColour = [...byName.values()].filter(
        (fold) =>
          !commanderNames.has(fold.card.name) &&
          fold.card.color_identity.some((colour) => !identity.has(colour)),
      )
      for (const fold of offColour) {
        for (const id of fold.ids)
          cardIssues.push({
            cardId: id,
            name: fold.card.name,
            status: 'off_colour',
            quantity: fold.copies,
          })
      }
      if (offColour.length > 0) {
        const names = joinNames([...new Set(commanders.map((card) => card.name))])
        violations.push({
          rule: 'colour-identity',
          severity: 'error',
          message:
            `${offColour.length} ${offColour.length === 1 ? 'card falls' : 'cards fall'} outside ` +
            `${names}'s colour identity (${identityLabel(identity)}).`,
        })
      }
    }
  }

  return { violations, cardIssues }
}

/** The command zone's own rules: how many cards it holds, and whether they may lead. */
function commandZoneViolations(zone: CommandZoneRule, commanders: Card[]): DeckRuleViolation[] {
  const violations: DeckRuleViolation[] = []

  if (zone.kind === 'oathbreaker') {
    // Two cards, and they must be one of each: the planeswalker and its signature spell.
    if (commanders.length < 2) {
      violations.push({
        rule: 'command-zone',
        severity: 'warning',
        message:
          'No oathbreaker and signature spell — put a legendary planeswalker and one ' +
          'instant or sorcery in a section named "Oathbreaker".',
      })
      return violations
    }
    if (commanders.length > 2) {
      violations.push({
        rule: 'command-zone',
        severity: 'error',
        message: `${commanders.length} cards in the command zone — an Oathbreaker deck has one oathbreaker and one signature spell.`,
      })
      return violations
    }
    const walkers = commanders.filter((card) => canLead(card, 'oathbreaker'))
    const spells = commanders.filter((card) => hasType(card, 'instant') || hasType(card, 'sorcery'))
    if (walkers.length !== 1 || spells.length !== 1) {
      violations.push({
        rule: 'commander-eligibility',
        severity: 'error',
        message: `${joinNames(commanders.map((card) => card.name))} can't lead the deck — an Oathbreaker deck needs exactly one legendary planeswalker and one instant or sorcery as its signature spell.`,
      })
    }
    return violations
  }

  if (commanders.length === 0) {
    violations.push({
      rule: 'command-zone',
      severity: 'warning',
      message: `No ${zone.noun} — put one in a section named "Commander".`,
    })
    return violations
  }

  const maxCommanders = zone.allowPairs ? 2 : 1
  if (commanders.length > maxCommanders) {
    violations.push({
      rule: 'command-zone',
      severity: 'error',
      message: zone.allowPairs
        ? `${commanders.length} cards in the command zone — a deck has one ${zone.noun}, or two that pair.`
        : `${commanders.length} cards in the command zone — a deck has one ${zone.noun}.`,
    })
  } else if (commanders.length === 2 && !pairAllowed(commanders[0]!, commanders[1]!)) {
    // Two copies of one card is a different mistake from two cards that don't pair, and
    // "X and X can't be commanders together" would read as nonsense.
    const [first, second] = commanders as [Card, Card]
    violations.push({
      rule: 'command-zone',
      severity: 'error',
      message:
        first.name === second.name
          ? `Two copies of ${first.name} in the command zone — a deck has one ${zone.noun}.`
          : `${joinNames([first.name, second.name])} can't be commanders together — a pair needs Partner, Friends forever, Doctor's companion, or Choose a Background.`,
    })
  }

  const ineligible = commanders.filter((card) => !canLead(card, zone.kind))
  if (ineligible.length > 0) {
    const names = joinNames([...new Set(ineligible.map((card) => card.name))])
    const allowed =
      zone.kind === 'brawl'
        ? 'a legendary creature or planeswalker'
        : zone.kind === 'pdh'
          ? 'an uncommon creature'
          : 'a legendary creature (or a card that says it can be your commander)'
    violations.push({
      rule: 'commander-eligibility',
      severity: 'error',
      message: `${names} can't be your ${zone.noun} — a ${zone.noun} must be ${allowed}.`,
    })
  }
  return violations
}
