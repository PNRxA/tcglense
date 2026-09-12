import type { CardDef } from './types'

/**
 * The simulator's card library: a handful of well-known cards whose rules text maps onto the
 * effects the engine models. Chosen so every teaching point has a card that shows it — a
 * burn spell and a counterspell for last-in-first-out, a pump spell for "fizzling", split
 * second, flash, a cast trigger, an enters trigger, a dies trigger, an activated ability and
 * two mana abilities. Oracle text is abridged to what the model supports.
 */
export const CARDS: readonly CardDef[] = [
  {
    id: 'lightning-bolt',
    name: 'Lightning Bolt',
    type: 'instant',
    manaCost: '{R}',
    text: 'Lightning Bolt deals 3 damage to any target.',
    target: { kind: 'any' },
    effect: { kind: 'damage', amount: 3 },
  },
  {
    id: 'shock',
    name: 'Shock',
    type: 'instant',
    manaCost: '{R}',
    text: 'Shock deals 2 damage to any target.',
    target: { kind: 'any' },
    effect: { kind: 'damage', amount: 2 },
  },
  {
    id: 'counterspell',
    name: 'Counterspell',
    type: 'instant',
    manaCost: '{U}{U}',
    text: 'Counter target spell.',
    target: { kind: 'spell' },
    effect: { kind: 'counter' },
  },
  {
    id: 'negate',
    name: 'Negate',
    type: 'instant',
    manaCost: '{1}{U}',
    text: 'Counter target noncreature spell.',
    target: { kind: 'noncreature-spell' },
    effect: { kind: 'counter' },
  },
  {
    id: 'stifle',
    name: 'Stifle',
    type: 'instant',
    manaCost: '{U}',
    text: 'Counter target activated or triggered ability.',
    target: { kind: 'ability' },
    effect: { kind: 'counter' },
  },
  {
    id: 'giant-growth',
    name: 'Giant Growth',
    type: 'instant',
    manaCost: '{G}',
    text: 'Target creature gets +3/+3 until end of turn.',
    target: { kind: 'creature' },
    effect: { kind: 'pump', power: 3, toughness: 3 },
  },
  {
    id: 'fork',
    name: 'Fork',
    type: 'instant',
    manaCost: '{R}{R}',
    text: 'Copy target instant or sorcery spell. (Not modelled: the real card also lets you choose new targets for the copy.)',
    target: { kind: 'instant-or-sorcery-spell' },
    effect: { kind: 'copy' },
  },
  {
    id: 'sudden-shock',
    name: 'Sudden Shock',
    type: 'instant',
    manaCost: '{1}{R}',
    text: 'Split second. Sudden Shock deals 2 damage to any target.',
    splitSecond: true,
    target: { kind: 'any' },
    effect: { kind: 'damage', amount: 2 },
  },
  {
    id: 'krosan-grip',
    name: 'Krosan Grip',
    type: 'instant',
    manaCost: '{2}{G}',
    text: 'Split second. Destroy target artifact or enchantment.',
    splitSecond: true,
    target: { kind: 'artifact-or-enchantment' },
    effect: { kind: 'destroy' },
  },
  {
    id: 'disenchant',
    name: 'Disenchant',
    type: 'instant',
    manaCost: '{1}{W}',
    text: 'Destroy target artifact or enchantment.',
    target: { kind: 'artifact-or-enchantment' },
    effect: { kind: 'destroy' },
  },
  {
    id: 'divination',
    name: 'Divination',
    type: 'sorcery',
    manaCost: '{2}{U}',
    text: 'Draw two cards.',
    effect: { kind: 'draw', count: 2 },
  },
  {
    id: 'wrath-of-god',
    name: 'Wrath of God',
    type: 'sorcery',
    manaCost: '{2}{W}{W}',
    text: 'Destroy all creatures.',
    effect: { kind: 'destroy-all-creatures' },
  },
  {
    id: 'grizzly-bears',
    name: 'Grizzly Bears',
    type: 'creature',
    manaCost: '{1}{G}',
    text: '',
    power: 2,
    toughness: 2,
  },
  {
    id: 'ambush-viper',
    name: 'Ambush Viper',
    type: 'creature',
    manaCost: '{1}{G}',
    text: 'Flash.',
    flash: true,
    power: 2,
    toughness: 1,
  },
  {
    id: 'llanowar-elves',
    name: 'Llanowar Elves',
    type: 'creature',
    manaCost: '{G}',
    text: '{T}: Add {G}.',
    power: 1,
    toughness: 1,
    activated: { text: '{T}: Add {G}.', taps: true, effect: { kind: 'none' }, mana: true },
  },
  {
    id: 'elvish-visionary',
    name: 'Elvish Visionary',
    type: 'creature',
    manaCost: '{1}{G}',
    text: 'When Elvish Visionary enters, draw a card.',
    power: 1,
    toughness: 1,
    triggers: [
      {
        event: 'self-enters',
        text: 'When Elvish Visionary enters, draw a card.',
        effect: { kind: 'draw', count: 1 },
      },
    ],
  },
  {
    id: 'soul-warden',
    name: 'Soul Warden',
    type: 'creature',
    manaCost: '{W}',
    text: 'Whenever another creature enters, you gain 1 life.',
    power: 1,
    toughness: 1,
    triggers: [
      {
        event: 'other-creature-enters',
        text: 'Whenever another creature enters, you gain 1 life.',
        effect: { kind: 'gain-life', amount: 1 },
      },
    ],
  },
  {
    id: 'guttersnipe',
    name: 'Guttersnipe',
    type: 'creature',
    manaCost: '{2}{R}',
    text: 'Whenever you cast an instant or sorcery spell, Guttersnipe deals 2 damage to each opponent.',
    power: 2,
    toughness: 2,
    triggers: [
      {
        event: 'cast-instant-or-sorcery',
        text: 'Whenever you cast an instant or sorcery spell, Guttersnipe deals 2 damage to each opponent.',
        effect: { kind: 'each-opponent-damage', amount: 2 },
      },
    ],
  },
  {
    id: 'zulaport-cutthroat',
    name: 'Zulaport Cutthroat',
    type: 'creature',
    manaCost: '{1}{B}',
    text: 'Whenever Zulaport Cutthroat or another creature you control dies, each opponent loses 1 life and you gain 1 life.',
    power: 1,
    toughness: 1,
    triggers: [
      {
        event: 'dies',
        text: 'Whenever Zulaport Cutthroat or another creature you control dies, each opponent loses 1 life and you gain 1 life.',
        effect: { kind: 'drain', amount: 1 },
      },
    ],
  },
  {
    id: 'prodigal-sorcerer',
    name: 'Prodigal Sorcerer',
    type: 'creature',
    manaCost: '{2}{U}',
    text: '{T}: Prodigal Sorcerer deals 1 damage to any target.',
    power: 1,
    toughness: 1,
    activated: {
      text: '{T}: Prodigal Sorcerer deals 1 damage to any target.',
      taps: true,
      target: { kind: 'any' },
      effect: { kind: 'damage', amount: 1 },
    },
  },
  {
    id: 'sol-ring',
    name: 'Sol Ring',
    type: 'artifact',
    manaCost: '{1}',
    text: '{T}: Add {C}{C}.',
    activated: { text: '{T}: Add {C}{C}.', taps: true, effect: { kind: 'none' }, mana: true },
  },
  {
    id: 'rhystic-study',
    name: 'Rhystic Study',
    type: 'enchantment',
    manaCost: '{2}{U}',
    text: 'Whenever an opponent casts a spell, you may draw a card unless that player pays {1}. (Not modelled: shown as a plain enchantment.)',
  },
]

const BY_ID: ReadonlyMap<string, CardDef> = new Map(CARDS.map((card) => [card.id, card]))

export function cardById(id: string): CardDef | undefined {
  return BY_ID.get(id)
}

/** The display order for the cast panel: what you respond with first, permanents last. */
export const CARD_TYPE_ORDER: readonly CardDef['type'][] = [
  'instant',
  'sorcery',
  'creature',
  'artifact',
  'enchantment',
]

export const CARD_TYPE_LABELS: Readonly<Record<CardDef['type'], string>> = {
  instant: 'Instants',
  sorcery: 'Sorceries',
  creature: 'Creatures',
  artifact: 'Artifacts',
  enchantment: 'Enchantments',
}
