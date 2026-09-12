import { cardById } from './cards'
import { nameOf, possessive, pushLog } from './state'
import { describeTarget, targetIsLegal } from './targets'
import type { CardDef, Permanent, PlayerId, StackObject, StackState, TriggerEvent } from './types'
import { otherPlayer, PLAYER_IDS } from './types'

/**
 * What happens to objects once the players stop acting: the top of the stack resolving,
 * permanents entering and leaving the battlefield, the abilities those events trigger, and
 * the state-based actions that tidy up before anyone gets priority again. Everything here
 * mutates the state it is handed — `engine.ts` owns the clone-per-action discipline.
 */

export function displayPower(permanent: Permanent): number | null {
  return permanent.power == null ? null : permanent.power + permanent.pumpPower
}

export function displayToughness(permanent: Permanent): number | null {
  return permanent.toughness == null ? null : permanent.toughness + permanent.pumpToughness
}

export function statsLabel(permanent: Permanent): string | null {
  const power = displayPower(permanent)
  const toughness = displayToughness(permanent)
  return power == null || toughness == null ? null : `${power}/${toughness}`
}

/** A triggered ability as an object waiting to be put on the stack (CR 603.2). */
function triggerObject(
  state: StackState,
  source: Permanent,
  trigger: NonNullable<CardDef['triggers']>[number],
): StackObject {
  return {
    id: state.nextId++,
    kind: 'triggered',
    name: `${source.name}'s trigger`,
    text: trigger.text,
    cardId: source.cardId,
    sourceId: source.id,
    controller: source.controller,
    effect: trigger.effect,
  }
}

/**
 * Queue every ability on the battlefield that triggers on `event`. `subject` is the permanent
 * the event is about (the one entering or dying); `actor` the player who cast, for cast
 * triggers. Leaves-the-battlefield abilities "look back": a dying permanent sees itself die.
 */
export function queueTriggers(
  state: StackState,
  event: TriggerEvent,
  context: { subject?: Permanent; actor?: PlayerId; snapshot?: Permanent[] },
): void {
  const permanents = context.snapshot ?? state.battlefield
  for (const permanent of permanents) {
    const card = cardById(permanent.cardId)
    for (const trigger of card?.triggers ?? []) {
      if (trigger.event !== event) continue
      const subject = context.subject
      let fires = false
      switch (event) {
        case 'self-enters':
          fires = subject?.id === permanent.id
          break
        case 'other-creature-enters':
          fires = !!subject && subject.id !== permanent.id && subject.type === 'creature'
          break
        case 'cast-instant-or-sorcery':
          fires = context.actor === permanent.controller
          break
        case 'dies':
          fires =
            !!subject && subject.type === 'creature' && subject.controller === permanent.controller
          break
      }
      if (!fires) continue
      state.pendingTriggers.push(triggerObject(state, permanent, trigger))
      pushLog(
        state,
        'trigger',
        `${permanent.name}'s ability triggers (${possessive(state, permanent.controller)}): "${trigger.text}" It waits until a player would receive priority.`,
        '603.2',
      )
    }
  }
}

/** `quiet` is the setup shortcut: the permanent was "always there", so nothing triggers. */
export function enterBattlefield(
  state: StackState,
  card: CardDef,
  controller: PlayerId,
  options: { quiet?: boolean } = {},
): Permanent {
  const permanent: Permanent = {
    id: state.nextId++,
    cardId: card.id,
    name: card.name,
    type: card.type,
    controller,
    power: card.power ?? null,
    toughness: card.toughness ?? null,
    damage: 0,
    pumpPower: 0,
    pumpToughness: 0,
    tapped: false,
  }
  state.battlefield.push(permanent)
  if (!options.quiet) {
    queueTriggers(state, 'self-enters', { subject: permanent })
    queueTriggers(state, 'other-creature-enters', { subject: permanent })
  }
  return permanent
}

/**
 * Remove permanents from the battlefield at once (`Wrath of God` is one event, so every
 * dies trigger sees every creature). `verb` is the log's word for what happened.
 */
export function leaveBattlefield(
  state: StackState,
  ids: number[],
  verb: string,
  rule?: string,
): void {
  const leaving = state.battlefield.filter((p) => ids.includes(p.id))
  if (leaving.length === 0) return
  const snapshot = [...state.battlefield]
  for (const permanent of leaving) {
    pushLog(
      state,
      'sba',
      `${permanent.name} (${possessive(state, permanent.controller)}) is ${verb} and goes to the graveyard.`,
      rule,
    )
  }
  state.battlefield = state.battlefield.filter((p) => !ids.includes(p.id))
  for (const permanent of leaving) {
    state.graveyard.push({ name: permanent.name, owner: permanent.controller })
    queueTriggers(state, 'dies', { subject: permanent, snapshot })
  }
}

function dealDamageToPlayer(
  state: StackState,
  source: string,
  player: PlayerId,
  amount: number,
): void {
  const before = state.players[player].life
  state.players[player].life = before - amount
  pushLog(
    state,
    'resolve',
    `${source} deals ${amount} damage to ${nameOf(state, player)} (${before} → ${before - amount}).`,
  )
}

function dealDamageToPermanent(
  state: StackState,
  source: string,
  permanent: Permanent,
  amount: number,
): void {
  permanent.damage += amount
  const stats = statsLabel(permanent)
  pushLog(
    state,
    'resolve',
    `${source} deals ${amount} damage to ${permanent.name}${stats ? ` (${stats})` : ''} — it now has ${permanent.damage} damage marked. Whether that is lethal is decided by state-based actions, checked before anyone gets priority.`,
  )
}

function applyEffect(state: StackState, object: StackObject): void {
  const { effect, target } = object
  const source = object.name
  switch (effect.kind) {
    case 'damage': {
      if (target?.kind === 'player') dealDamageToPlayer(state, source, target.id, effect.amount)
      else if (target?.kind === 'permanent') {
        const permanent = state.battlefield.find((p) => p.id === target.id)
        if (permanent) dealDamageToPermanent(state, source, permanent, effect.amount)
      }
      return
    }
    case 'counter': {
      if (target?.kind !== 'stack') return
      const index = state.stack.findIndex((o) => o.id === target.id)
      const countered = state.stack[index]
      if (!countered) return
      state.stack.splice(index, 1)
      if (countered.kind === 'spell') {
        state.graveyard.push({ name: countered.name, owner: countered.controller })
        const never =
          countered.cardType === 'instant' || countered.cardType === 'sorcery'
            ? 'its effect never happens'
            : 'it never enters the battlefield'
        pushLog(
          state,
          'resolve',
          `${source} resolves: ${countered.name} is countered. It is removed from the stack and put into ${possessive(state, countered.controller)} graveyard — ${never}.`,
          '701.5',
        )
      } else {
        pushLog(
          state,
          'resolve',
          `${source} resolves: ${countered.name} is countered and removed from the stack. ${countered.kind === 'copy' ? 'A copy simply ceases to exist.' : 'An ability has no card, so it simply ceases to exist — its source stays on the battlefield.'}`,
          '701.5',
        )
      }
      return
    }
    case 'pump': {
      if (target?.kind !== 'permanent') return
      const permanent = state.battlefield.find((p) => p.id === target.id)
      if (!permanent) return
      permanent.pumpPower += effect.power
      permanent.pumpToughness += effect.toughness
      pushLog(
        state,
        'resolve',
        `${source} resolves: ${permanent.name} gets +${effect.power}/+${effect.toughness} until end of turn (now ${statsLabel(permanent)}).`,
      )
      return
    }
    case 'destroy': {
      if (target?.kind !== 'permanent') return
      pushLog(state, 'resolve', `${source} resolves.`)
      leaveBattlefield(state, [target.id], 'destroyed')
      return
    }
    case 'destroy-all-creatures': {
      const ids = state.battlefield.filter((p) => p.type === 'creature').map((p) => p.id)
      pushLog(
        state,
        'resolve',
        ids.length
          ? `${source} resolves: every creature is destroyed at once.`
          : `${source} resolves, but there are no creatures to destroy.`,
      )
      leaveBattlefield(state, ids, 'destroyed')
      return
    }
    case 'draw':
      pushLog(
        state,
        'resolve',
        `${source} resolves: ${nameOf(state, object.controller)} draw${object.controller === 'you' ? '' : 's'} ${effect.count === 1 ? 'a card' : `${effect.count} cards`}. (The simulator keeps no hand — every card stays available to cast.)`,
      )
      return
    case 'gain-life': {
      const player = state.players[object.controller]
      player.life += effect.amount
      pushLog(
        state,
        'resolve',
        `${source} resolves: ${player.name} gain${object.controller === 'you' ? '' : 's'} ${effect.amount} life (now ${player.life}).`,
      )
      return
    }
    case 'each-opponent-damage': {
      pushLog(state, 'resolve', `${source} resolves.`)
      dealDamageToPlayer(state, source, otherPlayer(object.controller), effect.amount)
      return
    }
    case 'drain': {
      const opponent = state.players[otherPlayer(object.controller)]
      const controller = state.players[object.controller]
      opponent.life -= effect.amount
      controller.life += effect.amount
      pushLog(
        state,
        'resolve',
        `${source} resolves: ${opponent.name} loses ${effect.amount} life (now ${opponent.life}) and ${controller.name} gain${object.controller === 'you' ? '' : 's'} ${effect.amount} (now ${controller.life}).`,
      )
      return
    }
    case 'copy': {
      if (target?.kind !== 'stack') return
      const original = state.stack.find((o) => o.id === target.id)
      if (!original) return
      const copy: StackObject = {
        ...original,
        id: state.nextId++,
        kind: 'copy',
        name: `Copy of ${original.name}`,
        controller: object.controller,
      }
      state.stack.push(copy)
      pushLog(
        state,
        'resolve',
        `${source} resolves: a copy of ${original.name} is put on top of the stack under ${possessive(state, object.controller)} control, with the same target${original.target ? ` (${describeTarget(state, original.target)})` : ''}. The copy resolves before the original.`,
        '707.10',
      )
      return
    }
    case 'none':
      pushLog(state, 'resolve', `${source} resolves.`)
  }
}

/** Where an object goes after resolving or fizzling: graveyard, battlefield, or nowhere. */
function dispose(state: StackState, object: StackObject, resolved: boolean): void {
  if (object.kind === 'spell') {
    const card = cardById(object.cardId)
    const permanentSpell =
      object.cardType === 'creature' ||
      object.cardType === 'artifact' ||
      object.cardType === 'enchantment'
    if (resolved && permanentSpell && card) {
      pushLog(
        state,
        'resolve',
        `${object.name} resolves and enters the battlefield under ${possessive(state, object.controller)} control.`,
        '608.3',
      )
      enterBattlefield(state, card, object.controller)
      return
    }
    state.graveyard.push({ name: object.name, owner: object.controller })
    if (resolved) {
      pushLog(
        state,
        'resolve',
        `${object.name} is put into ${possessive(state, object.controller)} graveyard.`,
        '608.2',
      )
    }
    return
  }
  if (object.kind === 'copy') {
    pushLog(
      state,
      'resolve',
      `${object.name} ceases to exist — a copy never goes to a graveyard.`,
      '707.10',
    )
  }
}

/** Resolve the top of the stack (CR 608). Assumes the caller checked that everyone passed. */
export function resolveTop(state: StackState): void {
  const object = state.stack.pop()
  if (!object) return
  if (!targetIsLegal(state, object)) {
    const was = object.target ? describeTarget(state, object.target) : 'its target'
    pushLog(
      state,
      'resolve',
      `${object.name} would resolve, but its target is no longer legal (${was}). With no legal target it doesn't resolve at all — it "fizzles" and none of its effects happen.`,
      '608.2b',
    )
    dispose(state, object, false)
    if (object.kind === 'spell') {
      pushLog(
        state,
        'resolve',
        `${object.name} is put into ${possessive(state, object.controller)} graveyard without resolving.`,
      )
    }
    return
  }
  applyEffect(state, object)
  dispose(state, object, true)
}

/** One pass of state-based actions (CR 704). Returns whether anything happened. */
export function runStateBasedActions(state: StackState): boolean {
  let acted = false
  for (const player of PLAYER_IDS) {
    if (state.players[player].life <= 0 && state.loser === null) {
      state.loser = player
      acted = true
      pushLog(
        state,
        'sba',
        `State-based action: ${nameOf(state, player)} ${player === 'you' ? 'have' : 'has'} 0 or less life and lose${player === 'you' ? '' : 's'} the game.`,
        '704.5a',
      )
    }
  }
  const zeroToughness = state.battlefield.filter(
    (p) => p.type === 'creature' && (displayToughness(p) ?? 1) <= 0,
  )
  if (zeroToughness.length) {
    pushLog(
      state,
      'sba',
      'State-based action: a creature with toughness 0 or less is put into the graveyard.',
      '704.5f',
    )
    leaveBattlefield(
      state,
      zeroToughness.map((p) => p.id),
      'put into the graveyard for having 0 toughness',
      '704.5f',
    )
    acted = true
  }
  const lethal = state.battlefield.filter(
    (p) => p.type === 'creature' && p.damage > 0 && p.damage >= (displayToughness(p) ?? Infinity),
  )
  if (lethal.length) {
    for (const p of lethal) {
      pushLog(
        state,
        'sba',
        `State-based action: ${p.name} has ${p.damage} damage marked and toughness ${displayToughness(p)} — lethal damage.`,
        '704.5g',
      )
    }
    leaveBattlefield(
      state,
      lethal.map((p) => p.id),
      'destroyed',
      '704.5g',
    )
    acted = true
  }
  return acted
}

/**
 * Put the waiting triggers on the stack in APNAP order (CR 603.3b): the active player's first,
 * then the non-active player's — so the non-active player's resolve first.
 */
export function placePendingTriggers(state: StackState): boolean {
  if (state.pendingTriggers.length === 0) return false
  const active = state.pendingTriggers.filter((t) => t.controller === state.activePlayer)
  const passive = state.pendingTriggers.filter((t) => t.controller !== state.activePlayer)
  const ordered = [...active, ...passive]
  const bothPlayers = active.length > 0 && passive.length > 0
  if (bothPlayers) {
    pushLog(
      state,
      'trigger',
      `Abilities triggered for both players at once. APNAP order: ${nameOf(state, state.activePlayer)} (the active player) put${state.activePlayer === 'you' ? '' : 's'} theirs on the stack first, then ${nameOf(state, otherPlayer(state.activePlayer))} — so ${possessive(state, otherPlayer(state.activePlayer))} resolve first.`,
      '603.3b',
    )
  }
  for (const trigger of ordered) {
    state.stack.push(trigger)
    const below = state.stack[state.stack.length - 2]
    pushLog(
      state,
      'trigger',
      `${trigger.name} (${possessive(state, trigger.controller)}) is put on the stack${below ? ` above ${below.name}` : ''}.${below ? ' It resolves first.' : ''}`,
      '603.3',
    )
  }
  state.pendingTriggers = []
  state.passes = []
  return true
}
