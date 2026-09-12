/**
 * The stack simulator's data model.
 *
 * A deliberately small slice of the Magic rules: two players, a stack, a battlefield of
 * simple permanents, and the priority system that decides who may act and when the top of
 * the stack resolves. Mana, hands and libraries are out of scope — every card in the
 * library is always castable, because the tool teaches *ordering*, not resource management.
 * The model is plain data (no class instances, no functions) so a state can be cloned,
 * kept in an undo history and rendered without ceremony.
 */

export type PlayerId = 'you' | 'opp'

export const PLAYER_IDS: readonly PlayerId[] = ['you', 'opp'] as const

export function otherPlayer(player: PlayerId): PlayerId {
  return player === 'you' ? 'opp' : 'you'
}

export type CardType = 'instant' | 'sorcery' | 'creature' | 'artifact' | 'enchantment'

/** What a spell or ability may aim at when it is put on the stack. */
export type TargetSpec =
  | { kind: 'spell' }
  | { kind: 'noncreature-spell' }
  | { kind: 'instant-or-sorcery-spell' }
  | { kind: 'ability' }
  | { kind: 'creature' }
  | { kind: 'artifact-or-enchantment' }
  | { kind: 'any' }

/** What resolving does. One effect per object keeps the model legible in the log. */
export type Effect =
  | { kind: 'damage'; amount: number }
  | { kind: 'counter' }
  | { kind: 'pump'; power: number; toughness: number }
  | { kind: 'destroy' }
  | { kind: 'destroy-all-creatures' }
  | { kind: 'draw'; count: number }
  | { kind: 'gain-life'; amount: number }
  | { kind: 'each-opponent-damage'; amount: number }
  /** Each opponent loses `amount` life and the controller gains that much. */
  | { kind: 'drain'; amount: number }
  | { kind: 'copy' }
  | { kind: 'none' }

export type TriggerEvent =
  | 'self-enters'
  | 'other-creature-enters'
  | 'cast-instant-or-sorcery'
  | 'dies'

export interface TriggerDef {
  event: TriggerEvent
  text: string
  effect: Effect
}

export interface ActivatedDef {
  text: string
  /** `{T}` in the cost: the permanent taps, and can't be activated again until it untaps. */
  taps: boolean
  target?: TargetSpec
  effect: Effect
  /** A mana ability: resolves immediately and never uses the stack (CR 605.3). */
  mana?: boolean
}

export interface CardDef {
  id: string
  name: string
  type: CardType
  /** Display only — `{1}{U}` — the simulator never charges it. */
  manaCost: string
  /** The rules text as it reads on the card (abridged to what the model supports). */
  text: string
  power?: number
  toughness?: number
  flash?: boolean
  splitSecond?: boolean
  target?: TargetSpec
  /** Instants and sorceries resolve into this; permanents only if they have a spell effect. */
  effect?: Effect
  activated?: ActivatedDef
  triggers?: TriggerDef[]
}

export interface Player {
  id: PlayerId
  name: string
  life: number
}

export interface Permanent {
  id: number
  cardId: string
  name: string
  type: CardType
  controller: PlayerId
  power: number | null
  toughness: number | null
  damage: number
  /** "Until end of turn" pumps, cleared at cleanup. */
  pumpPower: number
  pumpToughness: number
  tapped: boolean
  /** The turn it came under its controller's control; 0 for a setup-placed one ("always there"). */
  enteredTurn: number
}

export type TargetRef =
  | { kind: 'stack'; id: number }
  | { kind: 'permanent'; id: number }
  | { kind: 'player'; id: PlayerId }

export type StackObjectKind = 'spell' | 'activated' | 'triggered' | 'copy'

export interface StackObject {
  id: number
  kind: StackObjectKind
  /** "Lightning Bolt", "Prodigal Sorcerer's ability", "Soul Warden's trigger". */
  name: string
  text: string
  cardId: string
  /** The permanent an ability came from (display only — the ability outlives its source). */
  sourceId?: number
  controller: PlayerId
  /** Spells only: what the card becomes when it resolves. */
  cardType?: CardType
  splitSecond?: boolean
  targetSpec?: TargetSpec
  target?: TargetRef
  effect: Effect
}

export type LogKind = 'action' | 'pass' | 'resolve' | 'trigger' | 'sba' | 'refused' | 'note'

export interface LogEntry {
  id: number
  kind: LogKind
  text: string
  /** The Comprehensive Rules paragraph this line rests on, keyed into `rules.ts`. */
  rule?: string
}

export interface StackState {
  players: Record<PlayerId, Player>
  activePlayer: PlayerId
  priority: PlayerId
  turn: number
  /** Bottom first — the last element is the top of the stack, the next to resolve. */
  stack: StackObject[]
  battlefield: Permanent[]
  graveyard: { name: string; owner: PlayerId }[]
  /** Abilities that have triggered but not yet been put on the stack (CR 603.3). */
  pendingTriggers: StackObject[]
  /** Players who have passed in succession since the stack last changed (CR 117.4). */
  passes: PlayerId[]
  /** Set once a state-based action has ended the game; every action is refused after. */
  loser: PlayerId | null
  nextId: number
  log: LogEntry[]
}

export type Action =
  | { type: 'cast'; player: PlayerId; cardId: string; target?: TargetRef }
  | { type: 'activate'; player: PlayerId; permanentId: number; target?: TargetRef }
  | { type: 'pass'; player: PlayerId }
  /** Setup shortcut: the permanent is put onto the battlefield without being cast. */
  | { type: 'put-onto-battlefield'; player: PlayerId; cardId: string }
