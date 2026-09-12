import { nameOf, possessive } from './state'
import type {
  Effect,
  Permanent,
  PlayerId,
  StackObject,
  StackState,
  TargetRef,
  TargetSpec,
} from './types'

/**
 * Targeting: which objects a spec may aim at right now, whether a chosen target is still
 * legal as its spell resolves, and how a target reads in the log. One module so the cast
 * panel, the engine's refusals and the resolution-time check can't drift on what "a spell"
 * or "a creature" means.
 */

/** A copy of a spell is a spell (CR 707.10), so it is a legal target for a counterspell. */
export function isSpell(object: StackObject): boolean {
  return object.kind === 'spell' || object.kind === 'copy'
}

export function isAbility(object: StackObject): boolean {
  return object.kind === 'activated' || object.kind === 'triggered'
}

function stackObjectMatches(object: StackObject, spec: TargetSpec): boolean {
  switch (spec.kind) {
    case 'spell':
      return isSpell(object)
    case 'noncreature-spell':
      return isSpell(object) && object.cardType !== 'creature'
    case 'instant-or-sorcery-spell':
      return isSpell(object) && (object.cardType === 'instant' || object.cardType === 'sorcery')
    case 'ability':
      return isAbility(object)
    default:
      return false
  }
}

function permanentMatches(permanent: Permanent, spec: TargetSpec): boolean {
  switch (spec.kind) {
    case 'creature':
    case 'any':
      return permanent.type === 'creature'
    case 'artifact-or-enchantment':
      return permanent.type === 'artifact' || permanent.type === 'enchantment'
    default:
      return false
  }
}

/** Every legal target for `spec`, players last. */
export function legalTargets(state: StackState, spec: TargetSpec): TargetRef[] {
  const refs: TargetRef[] = []
  for (const object of state.stack) {
    if (stackObjectMatches(object, spec)) refs.push({ kind: 'stack', id: object.id })
  }
  for (const permanent of state.battlefield) {
    if (permanentMatches(permanent, spec)) refs.push({ kind: 'permanent', id: permanent.id })
  }
  if (spec.kind === 'any') {
    for (const player of ['opp', 'you'] as const) refs.push({ kind: 'player', id: player })
  }
  return refs
}

/**
 * The same list, ordered by what `player` most plausibly wants first: a pump on their own
 * creature, anything else at the opponent's things. The cast panel preselects the first.
 */
export function preferredTargets(
  state: StackState,
  spec: TargetSpec,
  player: PlayerId,
  effect: Effect | undefined,
): TargetRef[] {
  const refs = legalTargets(state, spec)
  const helpful = effect?.kind === 'pump'
  const controllerOf = (ref: TargetRef): PlayerId | null => {
    if (ref.kind === 'player') return ref.id
    if (ref.kind === 'permanent') {
      return state.battlefield.find((p) => p.id === ref.id)?.controller ?? null
    }
    return state.stack.find((o) => o.id === ref.id)?.controller ?? null
  }
  const own = refs.filter((ref) => controllerOf(ref) === player)
  const theirs = refs.filter((ref) => controllerOf(ref) !== player)
  return helpful ? [...own, ...theirs] : [...theirs, ...own]
}

export function sameTarget(a: TargetRef, b: TargetRef): boolean {
  return a.kind === b.kind && a.id === b.id
}

/** Whether `object`'s chosen target is still there and still the kind of thing it may aim at. */
export function targetIsLegal(state: StackState, object: StackObject): boolean {
  if (!object.target || !object.targetSpec) return true
  const { target, targetSpec } = object
  switch (target.kind) {
    case 'stack': {
      const found = state.stack.find((o) => o.id === target.id)
      return !!found && stackObjectMatches(found, targetSpec)
    }
    case 'permanent': {
      const found = state.battlefield.find((p) => p.id === target.id)
      return !!found && permanentMatches(found, targetSpec)
    }
    case 'player':
      return state.loser !== target.id
  }
}

/** "Opponent", "Grizzly Bears (Opponent's)", "Lightning Bolt (on the stack)". */
export function describeTarget(state: StackState, ref: TargetRef): string {
  switch (ref.kind) {
    case 'player':
      return nameOf(state, ref.id)
    case 'permanent': {
      const found = state.battlefield.find((p) => p.id === ref.id)
      return found
        ? `${found.name} (${possessive(state, found.controller)})`
        : 'a permanent that is gone'
    }
    case 'stack': {
      const found = state.stack.find((o) => o.id === ref.id)
      return found ? `${found.name} (on the stack)` : 'a spell that has left the stack'
    }
  }
}

export const TARGET_SPEC_LABELS: Readonly<Record<TargetSpec['kind'], string>> = {
  spell: 'a spell on the stack',
  'noncreature-spell': 'a noncreature spell on the stack',
  'instant-or-sorcery-spell': 'an instant or sorcery spell on the stack',
  ability: 'an activated or triggered ability on the stack',
  creature: 'a creature',
  'artifact-or-enchantment': 'an artifact or enchantment',
  any: 'any target (a creature or a player)',
}
