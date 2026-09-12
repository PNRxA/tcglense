import { cardById } from './cards'
import {
  placePendingTriggers,
  queueTriggers,
  resolveTop,
  runStateBasedActions,
  enterBattlefield,
} from './resolve'
import { cloneState, hasSplitSecondOnStack, nameOf, possessive, pushLog } from './state'
import { legalTargets, sameTarget, describeTarget, TARGET_SPEC_LABELS } from './targets'
import type {
  Action,
  CardDef,
  Permanent,
  PlayerId,
  StackObject,
  StackState,
  TargetRef,
} from './types'
import { otherPlayer, PLAYER_IDS } from './types'

/**
 * The reducer: `applyAction` takes a state and a player's action and returns the next state
 * with the log extended by what happened and why. A refused action returns a state that
 * differs only by a `refused` log line, so the UI can show the reason without special cases.
 * Pure over its inputs — every call clones first — which is what makes undo a history of
 * states rather than a second implementation of every rule in reverse.
 */

export type Refusal = { ok: true } | { ok: false; reason: string; rule?: string }

/** Why `player` can't act at all right now, before looking at a particular card. */
function actionGate(state: StackState, player: PlayerId): Refusal {
  if (state.loser) {
    return { ok: false, reason: 'The game is over — undo a step or reset to keep exploring.' }
  }
  if (state.priority !== player) {
    return {
      ok: false,
      reason: `${nameOf(state, player)} ${player === 'you' ? "don't" : "doesn't"} have priority — ${nameOf(state, state.priority)} ${state.priority === 'you' ? 'do' : 'does'}. Only the player with priority may cast a spell or activate an ability.`,
      rule: '117.1',
    }
  }
  return { ok: true }
}

/** Whether `player` may cast `card` right now, and if not, the rule that stops them. */
export function castability(state: StackState, player: PlayerId, card: CardDef): Refusal {
  const gate = actionGate(state, player)
  if (!gate.ok) return gate
  if (hasSplitSecondOnStack(state)) {
    const spell = state.stack.find((o) => o.splitSecond)
    return {
      ok: false,
      reason: `${spell?.name ?? 'A spell'} has split second and is on the stack: while it is there, no player may cast spells or activate non-mana abilities.`,
      rule: '702.61',
    }
  }
  const instantSpeed = card.type === 'instant' || card.flash
  if (!instantSpeed) {
    const what = card.type === 'sorcery' ? 'a sorcery' : `a ${card.type} spell without flash`
    if (state.activePlayer !== player) {
      return {
        ok: false,
        reason: `${card.name} is ${what}: it can only be cast during ${possessive(state, player)} own main phase — and it is ${possessive(state, state.activePlayer)} turn.`,
        rule: '117.1',
      }
    }
    if (state.stack.length > 0) {
      return {
        ok: false,
        reason: `${card.name} is ${what}: it can only be cast while the stack is empty, and ${state.stack[state.stack.length - 1]?.name} is still waiting to resolve. Only instants, cards with flash and abilities can be cast in response.`,
        rule: '117.1',
      }
    }
  }
  if (card.target && legalTargets(state, card.target).length === 0) {
    return {
      ok: false,
      reason: `${card.name} needs ${TARGET_SPEC_LABELS[card.target.kind]}, and there is none. A spell that requires a target can't be cast without one.`,
      rule: '601.2',
    }
  }
  return { ok: true }
}

/** Whether `player` may activate `permanent`'s ability right now. */
export function activatability(state: StackState, player: PlayerId, permanent: Permanent): Refusal {
  const ability = cardById(permanent.cardId)?.activated
  if (!ability) return { ok: false, reason: `${permanent.name} has no activated ability.` }
  const gate = actionGate(state, player)
  if (!gate.ok) return gate
  if (permanent.controller !== player) {
    return {
      ok: false,
      reason: `${permanent.name} is ${possessive(state, permanent.controller)} — only its controller may activate its abilities.`,
    }
  }
  if (!ability.mana && hasSplitSecondOnStack(state)) {
    const spell = state.stack.find((o) => o.splitSecond)
    return {
      ok: false,
      reason: `${spell?.name ?? 'A spell'} has split second and is on the stack: only mana abilities may be activated while it is there.`,
      rule: '702.61',
    }
  }
  if (ability.taps && permanent.tapped) {
    return {
      ok: false,
      reason: `${permanent.name} is already tapped. It untaps at the start of ${possessive(state, permanent.controller)} next turn.`,
    }
  }
  if (ability.target && legalTargets(state, ability.target).length === 0) {
    return {
      ok: false,
      reason: `${permanent.name}'s ability needs ${TARGET_SPEC_LABELS[ability.target.kind]}, and there is none.`,
      rule: '602.2',
    }
  }
  return { ok: true }
}

function checkTarget(
  state: StackState,
  spec: CardDef['target'],
  target: TargetRef | undefined,
  name: string,
): Refusal {
  if (!spec) return { ok: true }
  if (!target)
    return {
      ok: false,
      reason: `${name} needs a target: choose ${TARGET_SPEC_LABELS[spec.kind]}.`,
      rule: '601.2',
    }
  if (!legalTargets(state, spec).some((ref) => sameTarget(ref, target))) {
    return {
      ok: false,
      reason: `${describeTarget(state, target)} isn't a legal target for ${name} — it needs ${TARGET_SPEC_LABELS[spec.kind]}.`,
      rule: '601.2',
    }
  }
  return { ok: true }
}

/**
 * Hand priority to `player` the way the rules do: state-based actions first, then waiting
 * triggers go on the stack, repeated until both are quiet (CR 117.5) — only then does the
 * player actually receive priority.
 */
function givePriority(state: StackState, player: PlayerId): void {
  for (let round = 0; round < 20; round++) {
    const acted = runStateBasedActions(state)
    const placed = placePendingTriggers(state)
    if (!acted && !placed) break
  }
  state.priority = player
}

function cast(
  state: StackState,
  player: PlayerId,
  cardId: string,
  target: TargetRef | undefined,
): void {
  const card = cardById(cardId)
  if (!card) {
    pushLog(state, 'refused', `Unknown card "${cardId}".`)
    return
  }
  const allowed = castability(state, player, card)
  if (!allowed.ok) {
    pushLog(
      state,
      'refused',
      `${nameOf(state, player)} can't cast ${card.name}: ${allowed.reason}`,
      allowed.rule,
    )
    return
  }
  const targetCheck = checkTarget(state, card.target, target, card.name)
  if (!targetCheck.ok) {
    pushLog(
      state,
      'refused',
      `${nameOf(state, player)} can't cast ${card.name}: ${targetCheck.reason}`,
      targetCheck.rule,
    )
    return
  }
  const object: StackObject = {
    id: state.nextId++,
    kind: 'spell',
    name: card.name,
    text: card.text,
    cardId: card.id,
    controller: player,
    cardType: card.type,
    effect: card.effect ?? { kind: 'none' },
    ...(card.splitSecond ? { splitSecond: true } : {}),
    ...(card.target ? { targetSpec: card.target } : {}),
    ...(target ? { target } : {}),
  }
  const below = state.stack[state.stack.length - 1]
  state.stack.push(object)
  state.passes = []
  const aimed = target ? ` targeting ${describeTarget(state, target)}` : ''
  const where = below
    ? ` on top of ${below.name} — a response resolves before what it responds to`
    : ' on the empty stack'
  pushLog(
    state,
    'action',
    `${nameOf(state, player)} cast${player === 'you' ? '' : 's'} ${card.name}${aimed}. It goes${where}. Nothing happens yet: a spell does nothing until it resolves.`,
    below ? '405.1' : '601.2',
  )
  if (card.flash && state.stack.length > 1) {
    pushLog(
      state,
      'note',
      `${card.name} has flash, so it can be cast whenever an instant could — including in response.`,
      '702.8',
    )
  }
  if (card.splitSecond) {
    pushLog(
      state,
      'note',
      `${card.name} has split second: while it is on the stack, nobody can cast spells or activate non-mana abilities. Triggered abilities and mana abilities still work.`,
      '702.61',
    )
  }
  if (card.type === 'instant' || card.type === 'sorcery') {
    queueTriggers(state, 'cast-instant-or-sorcery', { actor: player })
  }
  givePriority(state, player)
  pushLog(
    state,
    'note',
    `${nameOf(state, player)} keep${player === 'you' ? '' : 's'} priority after casting, and may respond to ${player === 'you' ? 'your' : 'their'} own spell or pass.`,
    '117.3c',
  )
}

function activate(
  state: StackState,
  player: PlayerId,
  permanentId: number,
  target: TargetRef | undefined,
): void {
  const permanent = state.battlefield.find((p) => p.id === permanentId)
  if (!permanent) {
    pushLog(state, 'refused', 'That permanent is no longer on the battlefield.')
    return
  }
  const ability = cardById(permanent.cardId)?.activated
  const allowed = activatability(state, player, permanent)
  if (!allowed.ok || !ability) {
    pushLog(
      state,
      'refused',
      `${nameOf(state, player)} can't activate ${permanent.name}: ${allowed.ok ? 'it has no ability.' : allowed.reason}`,
      allowed.ok ? undefined : allowed.rule,
    )
    return
  }
  const targetCheck = checkTarget(state, ability.target, target, `${permanent.name}'s ability`)
  if (!targetCheck.ok) {
    pushLog(
      state,
      'refused',
      `${nameOf(state, player)} can't activate ${permanent.name}: ${targetCheck.reason}`,
      targetCheck.rule,
    )
    return
  }
  if (ability.taps) permanent.tapped = true
  if (ability.mana) {
    const hadPasses = state.passes.length > 0
    state.passes = []
    pushLog(
      state,
      'action',
      `${nameOf(state, player)} tap${player === 'you' ? '' : 's'} ${permanent.name} for mana. A mana ability never uses the stack: it resolves immediately and can't be targeted, countered or responded to.${hadPasses ? ' It still counts as taking an action, so both players must pass again before the stack resolves.' : ''}`,
      '605.3',
    )
    return
  }
  const object: StackObject = {
    id: state.nextId++,
    kind: 'activated',
    name: `${permanent.name}'s ability`,
    text: ability.text,
    cardId: permanent.cardId,
    sourceId: permanent.id,
    controller: player,
    effect: ability.effect,
    ...(ability.target ? { targetSpec: ability.target } : {}),
    ...(target ? { target } : {}),
  }
  const below = state.stack[state.stack.length - 1]
  state.stack.push(object)
  state.passes = []
  const aimed = target ? ` targeting ${describeTarget(state, target)}` : ''
  pushLog(
    state,
    'action',
    `${nameOf(state, player)} activate${player === 'you' ? '' : 's'} ${permanent.name}'s ability${aimed}${ability.taps ? ', tapping it' : ''}. The ability goes on the stack${below ? ` above ${below.name}` : ''} just like a spell would, and can be responded to.`,
    '602.2',
  )
  givePriority(state, player)
  pushLog(
    state,
    'note',
    `${nameOf(state, player)} keep${player === 'you' ? '' : 's'} priority after activating.`,
    '117.3c',
  )
}

/** The phase ends and the simulator jumps to the other player's main phase. */
function nextTurn(state: StackState): void {
  pushLog(
    state,
    'pass',
    'Both players passed with an empty stack, so the phase ends. The simulator skips ahead to the next player’s main phase.',
    '500.2',
  )
  const wornOff = state.battlefield.filter(
    (p) => p.damage > 0 || p.pumpPower !== 0 || p.pumpToughness !== 0,
  )
  for (const permanent of state.battlefield) {
    permanent.damage = 0
    permanent.pumpPower = 0
    permanent.pumpToughness = 0
  }
  if (wornOff.length) {
    pushLog(
      state,
      'note',
      `Cleanup: damage is removed from ${wornOff.map((p) => p.name).join(', ')} and every "until end of turn" effect ends.`,
      '514.2',
    )
  }
  state.turn += 1
  state.activePlayer = otherPlayer(state.activePlayer)
  for (const permanent of state.battlefield) {
    if (permanent.controller === state.activePlayer) permanent.tapped = false
  }
  pushLog(
    state,
    'pass',
    `Turn ${state.turn}: it is now ${possessive(state, state.activePlayer)} turn. ${nameOf(state, state.activePlayer)} untap${state.activePlayer === 'you' ? '' : 's'} and, being the active player, receive${state.activePlayer === 'you' ? '' : 's'} priority first.`,
  )
  state.passes = []
  givePriority(state, state.activePlayer)
}

function pass(state: StackState, player: PlayerId): void {
  const gate = actionGate(state, player)
  if (!gate.ok) {
    pushLog(state, 'refused', `${nameOf(state, player)} can't pass: ${gate.reason}`, gate.rule)
    return
  }
  state.passes.push(player)
  const everyone = PLAYER_IDS.every((id) => state.passes.includes(id))
  if (!everyone) {
    const next = otherPlayer(player)
    pushLog(
      state,
      'pass',
      `${nameOf(state, player)} pass${player === 'you' ? '' : 'es'} priority. ${nameOf(state, next)} receive${next === 'you' ? '' : 's'} priority${state.stack.length ? ` and may respond to ${state.stack[state.stack.length - 1]?.name} or pass` : ''}.`,
      '117.3d',
    )
    givePriority(state, next)
    return
  }
  const top = state.stack[state.stack.length - 1]
  if (!top) {
    nextTurn(state)
    return
  }
  pushLog(
    state,
    'pass',
    `${nameOf(state, player)} pass${player === 'you' ? '' : 'es'} too. Both players passed in succession without acting, so the top of the stack resolves: ${top.name}.`,
    '117.4',
  )
  resolveTop(state)
  state.passes = []
  pushLog(
    state,
    'note',
    `After a resolution, priority goes to the active player, ${nameOf(state, state.activePlayer)} — regardless of who cast what just resolved.`,
    '117.3b',
  )
  givePriority(state, state.activePlayer)
}

function putOntoBattlefield(state: StackState, player: PlayerId, cardId: string): void {
  const card = cardById(cardId)
  if (!card || card.type === 'instant' || card.type === 'sorcery') {
    pushLog(
      state,
      'refused',
      `${card?.name ?? cardId} isn't a permanent card, so it can't be put onto the battlefield.`,
    )
    return
  }
  if (state.loser) {
    pushLog(state, 'refused', 'The game is over — undo a step or reset to keep exploring.')
    return
  }
  enterBattlefield(state, card, player, { quiet: true })
  pushLog(
    state,
    'note',
    `Setup: ${card.name} is put directly onto the battlefield under ${possessive(state, player)} control, as if it had been there all along — nothing is cast and nothing triggers.`,
  )
}

export function applyAction(previous: StackState, action: Action): StackState {
  const state = cloneState(previous)
  switch (action.type) {
    case 'cast':
      cast(state, action.player, action.cardId, action.target)
      break
    case 'activate':
      activate(state, action.player, action.permanentId, action.target)
      break
    case 'pass':
      pass(state, action.player)
      break
    case 'put-onto-battlefield':
      putOntoBattlefield(state, action.player, action.cardId)
      break
  }
  return state
}

/** Whether the last action was refused — the UI surfaces that line as the error, not history. */
export function lastRefusal(state: StackState): string | null {
  const last = state.log[state.log.length - 1]
  return last?.kind === 'refused' ? last.text : null
}
