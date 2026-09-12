import { hasSplitSecondOnStack, nameOf } from './state'
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
  // "You" mid-sentence reads as a name; the pronoun wants lower case there.
  const holderMid = state.priority === 'you' ? 'you' : holder
  const other = nameOf(state, otherPlayer(state.priority))
  const has = state.priority === 'you' ? 'have' : 'has'
  const top = state.stack[state.stack.length - 1]

  if (!top) {
    const timing =
      state.priority === state.activePlayer
        ? `It is ${state.priority === 'you' ? 'your' : 'their'} turn and the stack is empty, so anything can be cast — sorceries and creatures included.`
        : `It is ${state.priority === 'you' ? 'the opponent’s' : 'your'} turn, so only instants, cards with flash and abilities may be used.`
    return {
      text: `The stack is empty and ${holderMid} ${has} priority. ${timing} If both players pass with the stack empty, the phase ends and the turn moves on.`,
      rule: '117.1',
    }
  }

  const splitSecond = hasSplitSecondOnStack(state)
    ? ' A spell with split second is on the stack, so no spells or non-mana abilities can be added — only mana abilities and triggers.'
    : ''

  if (state.passes.length === 0) {
    return {
      text: `${holder} ${has} priority. ${top.name} is on top of the stack, waiting. If ${holderMid} pass${state.priority === 'you' ? '' : 'es'} and ${other} passes too, ${top.name} resolves. Either player may instead respond: a new spell or ability goes on top of ${top.name} and resolves first.${splitSecond}`,
      rule: '117.4',
    }
  }
  const passed = state.passes.map((id) => nameOf(state, id)).join(' and ')
  return {
    text: `${passed} passed. ${holder} now ${has} priority: pass as well and ${top.name} resolves; act instead and both players must pass again before anything resolves.${splitSecond}`,
    rule: '117.4',
  }
}

/** The ordinal each stack object resolves in, top first, for the column's labels. */
export function resolveOrderLabel(position: number, total: number): string {
  const fromTop = total - position
  if (fromTop === 1) return 'Resolves first'
  if (fromTop === 2) return 'Resolves second'
  if (fromTop === 3) return 'Resolves third'
  return `Resolves ${fromTop}th`
}
