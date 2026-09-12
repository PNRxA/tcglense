import type { LogEntry, LogKind, PlayerId, StackState } from './types'

/** The bookkeeping every part of the engine shares: building, cloning and logging a state. */

export interface CreateStateOptions {
  activePlayer?: PlayerId
  life?: number
}

export function createState(options: CreateStateOptions = {}): StackState {
  const life = options.life ?? 20
  const active = options.activePlayer ?? 'you'
  return {
    players: {
      you: { id: 'you', name: 'You', life },
      opp: { id: 'opp', name: 'Opponent', life },
    },
    activePlayer: active,
    priority: active,
    turn: 1,
    stack: [],
    battlefield: [],
    graveyard: [],
    pendingTriggers: [],
    passes: [],
    loser: null,
    nextId: 1,
    log: [],
  }
}

/**
 * A deep copy of plain data. The state is JSON-shaped by construction (no dates, functions or
 * undefined-bearing fields that matter), so this is both the simplest and the most portable
 * clone — it works the same in the browser and in jsdom.
 */
export function cloneState(state: StackState): StackState {
  return JSON.parse(JSON.stringify(state)) as StackState
}

export function pushLog(state: StackState, kind: LogKind, text: string, rule?: string): LogEntry {
  const entry: LogEntry = { id: state.nextId++, kind, text, ...(rule ? { rule } : {}) }
  state.log.push(entry)
  return entry
}

export function nameOf(state: StackState, player: PlayerId): string {
  return state.players[player].name
}

/** The determiner possessive: "your graveyard" / "Opponent's graveyard". */
export function possessive(state: StackState, player: PlayerId): string {
  return player === 'you' ? 'your' : `${nameOf(state, player)}'s`
}

/** The standalone possessive for a parenthetical tag: "(yours)" / "(Opponent's)". */
export function ownerTag(state: StackState, player: PlayerId): string {
  return player === 'you' ? 'yours' : `${nameOf(state, player)}'s`
}

/** The mid-sentence subject: the pronoun wants lower case, a name keeps its capital. */
export function subjectMid(state: StackState, player: PlayerId): string {
  return player === 'you' ? 'you' : nameOf(state, player)
}

export function hasSplitSecondOnStack(state: StackState): boolean {
  return state.stack.some((object) => object.splitSecond)
}
