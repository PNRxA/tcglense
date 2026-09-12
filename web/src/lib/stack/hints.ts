import { hasSplitSecondOnStack, nameOf, subjectMid } from './state'
import type { StackState } from './types'
import { otherPlayer } from './types'

/**
 * "What happens next": one paragraph describing the current situation and what each choice
 * would do, so the player is never left guessing why a button is there. Pure over the state,
 * like the log — the difference is that this is about the future, the log about the past.
 */
export interface Hint {
  text: string
  rule?: string
}

export function nextHint(state: StackState): Hint {
  if (state.loser) {
    return {
      text: `The game is over: ${nameOf(state, state.loser)} lost. Undo a step or reset the table to keep exploring.`,
      rule: '704.5a',
    }
  }
  const holder = nameOf(state, state.priority)
  // "You" mid-sentence reads as a name; the pronoun wants lower case there, and its verb agrees.
  const holderMid = subjectMid(state, state.priority)
  const otherId = otherPlayer(state.priority)
  const otherMid = subjectMid(state, otherId)
  const has = state.priority === 'you' ? 'have' : 'has'
  const holderPasses = state.priority === 'you' ? 'pass' : 'passes'
  const otherPasses = otherId === 'you' ? 'pass' : 'passes'
  const top = state.stack[state.stack.length - 1]

  if (!top) {
    const timing =
      state.priority === state.activePlayer
        ? `It is ${state.priority === 'you' ? 'your' : 'their'} turn and the stack is empty, so anything can be cast — sorceries and creatures included.`
        : `It is ${state.priority === 'you' ? 'the opponent’s' : 'your'} turn, and ${holderMid} ${state.priority === 'you' ? 'are' : 'is'} not the active player, so ${holderMid} may only use instants, cards with flash and abilities.`
    return {
      text: `The stack is empty and ${holderMid} ${has} priority. ${timing} If both players pass with the stack empty, the phase ends and the turn moves on.`,
      rule: '117.1',
    }
  }

  const splitSecond = hasSplitSecondOnStack(state)
    ? ' A spell with split second is on the stack, so no player can cast a spell or activate a non-mana ability — only triggered abilities still go on the stack, and mana abilities still work without ever using it.'
    : ''

  if (state.passes.length === 0) {
    return {
      text: `${holder} ${has} priority. ${top.name} is on top of the stack, waiting. If ${holderMid} ${holderPasses} and ${otherMid} ${otherPasses} too, ${top.name} resolves. Either player may instead respond: a new spell or ability goes on top of ${top.name} and resolves first.${splitSecond}`,
      rule: '117.4',
    }
  }
  const passed = state.passes.map((id) => nameOf(state, id)).join(' and ')
  return {
    text: `${passed} passed. ${holder} now ${has} priority: pass as well and ${top.name} resolves; act instead and both players must pass again before anything resolves.${splitSecond}`,
    rule: '117.4',
  }
}

/** English ordinal suffix: 1st/2nd/3rd/4th, with the 11th–13th exception. */
function ordinalSuffix(n: number): string {
  const tens = n % 100
  if (tens >= 11 && tens <= 13) return 'th'
  const last = n % 10
  if (last === 1) return 'st'
  if (last === 2) return 'nd'
  if (last === 3) return 'rd'
  return 'th'
}

/** The ordinal each stack object resolves in, top first, for the column's labels. */
export function resolveOrderLabel(position: number, total: number): string {
  const fromTop = total - position
  if (fromTop === 1) return 'Resolves first'
  if (fromTop === 2) return 'Resolves second'
  if (fromTop === 3) return 'Resolves third'
  return `Resolves ${fromTop}${ordinalSuffix(fromTop)}`
}
