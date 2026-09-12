import { applyAction } from './engine'
import { createState } from './state'
import type { Action, PlayerId, StackState, TargetRef } from './types'

/**
 * Guided walkthroughs: a starting table plus a script of actions, each with the commentary a
 * teacher would give before it. The engine is the only thing that changes the state — a
 * scenario's script is just the actions a player would take, so every walkthrough exercises
 * the same rules the free-play mode does, and a rule change can't leave a scenario telling a
 * different story than the log.
 */
export interface ScenarioStep {
  /** What to watch for, shown before the action runs. */
  say: string
  /** The action, resolved against the live state so a script can point at "the top spell". */
  act: (state: StackState) => Action
}

export interface Scenario {
  slug: string
  title: string
  /** The lesson, shown once the script is loaded. */
  lesson: string
  setup: () => StackState
  steps: ScenarioStep[]
}

/** Put permanents onto the battlefield quietly (no log) to stage a scenario. */
function stage(state: StackState, permanents: Array<[PlayerId, string]>): StackState {
  let next = state
  for (const [player, cardId] of permanents) {
    next = applyAction(next, { type: 'put-onto-battlefield', player, cardId })
  }
  next.log = []
  return next
}

/** `controller`'s permanent from card `cardId` — the script's way of pointing at a creature. */
function permanent(
  state: StackState,
  controller: PlayerId,
  cardId: string,
): { kind: 'permanent'; id: number } {
  const found = state.battlefield.find((p) => p.cardId === cardId && p.controller === controller)
  return { kind: 'permanent', id: found?.id ?? -1 }
}

/** The topmost object on the stack (a `-1` id if empty, which the engine refuses cleanly). */
function top(state: StackState): TargetRef {
  return { kind: 'stack', id: state.stack[state.stack.length - 1]?.id ?? -1 }
}

/** The newest spell on the stack from card `cardId`. */
function spell(state: StackState, cardId: string): TargetRef {
  const found = [...state.stack].reverse().find((o) => o.cardId === cardId && o.kind === 'spell')
  return { kind: 'stack', id: found?.id ?? -1 }
}

const player = (id: PlayerId): TargetRef => ({ kind: 'player', id })

const pass =
  (who: PlayerId): ScenarioStep['act'] =>
  () => ({ type: 'pass', player: who })
const cast =
  (who: PlayerId, cardId: string, target?: (state: StackState) => TargetRef): ScenarioStep['act'] =>
  (state) => ({ type: 'cast', player: who, cardId, ...(target ? { target: target(state) } : {}) })

export const SCENARIOS: readonly Scenario[] = [
  {
    slug: 'last-in-first-out',
    title: 'Last in, first out',
    lesson:
      'The stack resolves from the top down. A response is put on top of what it responds to, so it always resolves first — and both players must pass in succession before each object resolves.',
    setup: () => createState(),
    steps: [
      {
        say: 'You cast Lightning Bolt at the opponent. It goes on the stack and does nothing yet; you get priority back.',
        act: cast('you', 'lightning-bolt', () => player('opp')),
      },
      {
        say: 'You have nothing to add, so you pass. Passing hands priority to the opponent — the Bolt does not resolve yet, because the opponent has not passed.',
        act: pass('you'),
      },
      {
        say: 'The opponent responds with Counterspell targeting Lightning Bolt. It goes on top of the Bolt. Note that your earlier pass no longer counts: everyone must pass again.',
        act: cast('opp', 'counterspell', (s) => spell(s, 'lightning-bolt')),
      },
      {
        say: 'The opponent passes. Priority comes back to you: you could respond to the Counterspell now.',
        act: pass('opp'),
      },
      {
        say: 'You pass too. Both players passed in succession, so the top object — Counterspell — resolves, and the Bolt is countered.',
        act: pass('you'),
      },
    ],
  },
  {
    slug: 'counter-war',
    title: 'The counter war',
    lesson:
      'Each object on the stack resolves separately, with a round of priority between each. Countering a counterspell leaves the original spell on the stack, and it resolves afterwards as if nothing had happened.',
    setup: () => createState(),
    steps: [
      {
        say: 'You cast Lightning Bolt at the opponent and pass.',
        act: cast('you', 'lightning-bolt', () => player('opp')),
      },
      { say: 'Pass priority to the opponent.', act: pass('you') },
      {
        say: 'The opponent responds with Counterspell on the Bolt.',
        act: cast('opp', 'counterspell', (s) => spell(s, 'lightning-bolt')),
      },
      {
        say: 'The opponent passes; you get priority with their Counterspell on top.',
        act: pass('opp'),
      },
      {
        say: 'You respond with Negate, targeting the Counterspell (a noncreature spell). Three objects on the stack now: Bolt at the bottom, Counterspell, Negate on top.',
        act: cast('you', 'negate', (s) => spell(s, 'counterspell')),
      },
      { say: 'You pass.', act: pass('you') },
      {
        say: 'The opponent passes. Negate resolves: Counterspell is countered. Priority returns to you (the active player) with the Bolt still on the stack.',
        act: pass('opp'),
      },
      { say: 'You pass again.', act: pass('you') },
      {
        say: 'The opponent passes again — a second full round of passing — and now Lightning Bolt resolves.',
        act: pass('opp'),
      },
    ],
  },
  {
    slug: 'fizzle',
    title: 'Responding to a pump spell ("fizzling")',
    lesson:
      'A spell checks its targets again as it starts to resolve. If they are all gone, it does not resolve at all ("fizzles"). The response resolves first, kills the creature, and the pump spell finds nothing to pump.',
    setup: () => stage(createState(), [['you', 'grizzly-bears']]),
    steps: [
      {
        say: 'You cast Giant Growth on your Grizzly Bears.',
        act: cast('you', 'giant-growth', (s) => permanent(s, 'you', 'grizzly-bears')),
      },
      { say: 'You pass priority.', act: pass('you') },
      {
        say: 'The opponent responds with Lightning Bolt targeting the Bears. The Bolt is on top, so it resolves before the Growth.',
        act: cast('opp', 'lightning-bolt', (s) => permanent(s, 'you', 'grizzly-bears')),
      },
      { say: 'The opponent passes.', act: pass('opp') },
      {
        say: 'You pass. The Bolt resolves and deals 3 damage to a 2/2 — state-based actions destroy the Bears before anyone gets priority.',
        act: pass('you'),
      },
      { say: 'You pass with Giant Growth still on the stack.', act: pass('you') },
      {
        say: 'The opponent passes. Giant Growth tries to resolve, but its only target is gone: it fizzles. Compare with casting the Growth in response to the Bolt instead — try it in free play.',
        act: pass('opp'),
      },
    ],
  },
  {
    slug: 'save-the-bear',
    title: 'Saving a creature in response',
    lesson:
      'Order is everything. Cast the pump spell in response to the burn spell and it resolves first: the creature is 5/5 when the 3 damage arrives, so it survives.',
    setup: () => stage(createState({ activePlayer: 'opp' }), [['you', 'grizzly-bears']]),
    steps: [
      {
        say: 'It is the opponent’s turn. They cast Lightning Bolt at your Grizzly Bears.',
        act: cast('opp', 'lightning-bolt', (s) => permanent(s, 'you', 'grizzly-bears')),
      },
      { say: 'The opponent passes.', act: pass('opp') },
      {
        say: 'You respond with Giant Growth on the Bears. Being an instant, it can be cast on the opponent’s turn.',
        act: cast('you', 'giant-growth', (s) => permanent(s, 'you', 'grizzly-bears')),
      },
      { say: 'You pass.', act: pass('you') },
      {
        say: 'The opponent passes. Giant Growth resolves first: the Bears are 5/5 until end of turn.',
        act: pass('opp'),
      },
      { say: 'The opponent (active player) passes again.', act: pass('opp') },
      {
        say: 'You pass. Lightning Bolt resolves: 3 damage on a 5/5 is not lethal. The Bears live.',
        act: pass('you'),
      },
    ],
  },
  {
    slug: 'cast-trigger',
    title: 'A trigger on casting',
    lesson:
      'Triggered abilities are put on the stack the next time a player would receive priority. A "whenever you cast" trigger therefore lands on top of the spell itself and resolves before it.',
    setup: () => stage(createState(), [['you', 'guttersnipe']]),
    steps: [
      {
        say: 'You cast Shock at the opponent. Guttersnipe triggers, and the trigger goes on the stack above Shock before you receive priority.',
        act: cast('you', 'shock', () => player('opp')),
      },
      { say: 'You pass.', act: pass('you') },
      {
        say: 'The opponent passes. The trigger resolves first: 2 damage. Shock is still waiting.',
        act: pass('opp'),
      },
      { say: 'You pass again.', act: pass('you') },
      { say: 'The opponent passes again, and now Shock resolves.', act: pass('opp') },
    ],
  },
  {
    slug: 'apnap',
    title: 'APNAP: two players’ triggers at once',
    lesson:
      'When abilities trigger for both players at the same time, the active player puts theirs on the stack first, then the non-active player. Last in, first out: the non-active player’s trigger resolves first.',
    setup: () =>
      stage(createState(), [
        ['you', 'soul-warden'],
        ['opp', 'soul-warden'],
      ]),
    steps: [
      {
        say: 'It is your turn. You cast Grizzly Bears (a creature — allowed, since the stack is empty on your main phase).',
        act: cast('you', 'grizzly-bears'),
      },
      { say: 'You pass.', act: pass('you') },
      {
        say: 'The opponent passes. The Bears resolve and enter; both Soul Wardens trigger. Yours goes on the stack first (you are the active player), then the opponent’s on top.',
        act: pass('opp'),
      },
      { say: 'You pass.', act: pass('you') },
      { say: 'The opponent passes: their trigger, on top, resolves first.', act: pass('opp') },
      { say: 'You pass.', act: pass('you') },
      { say: 'The opponent passes: now your trigger resolves.', act: pass('opp') },
    ],
  },
  {
    slug: 'split-second',
    title: 'Split second',
    lesson:
      'While a split-second spell is on the stack, nobody can cast spells or activate non-mana abilities. Triggered abilities still trigger and still go on the stack, and mana abilities still work.',
    setup: () =>
      stage(createState(), [
        ['opp', 'sol-ring'],
        ['you', 'guttersnipe'],
        ['opp', 'llanowar-elves'],
      ]),
    steps: [
      {
        say: 'You cast Krosan Grip targeting the opponent’s Sol Ring. Guttersnipe still triggers — split second doesn’t stop triggers — and its trigger goes on the stack above the Grip.',
        act: cast('you', 'krosan-grip', (s) => permanent(s, 'opp', 'sol-ring')),
      },
      { say: 'You pass.', act: pass('you') },
      {
        say: 'The opponent tries to cast Counterspell on the Grip. Refused: split second.',
        act: cast('opp', 'counterspell', (s) => spell(s, 'krosan-grip')),
      },
      {
        say: 'The opponent taps Llanowar Elves for mana instead — a mana ability is still allowed, and it never touches the stack.',
        act: (s) => ({
          type: 'activate',
          player: 'opp',
          permanentId: permanent(s, 'opp', 'llanowar-elves').id,
        }),
      },
      {
        say: 'The opponent passes. Tapping for mana still counted as an action, so your earlier pass no longer counts: priority comes back to you.',
        act: pass('opp'),
      },
      { say: 'You pass. Both passed in succession: the trigger resolves first.', act: pass('you') },
      { say: 'You (the active player) pass again.', act: pass('you') },
      { say: 'The opponent passes, and Krosan Grip destroys the Sol Ring.', act: pass('opp') },
    ],
  },
  {
    slug: 'sorcery-timing',
    title: 'Sorcery timing',
    lesson:
      'Sorceries, creatures, artifacts and enchantments (without flash) can only be cast during your own main phase while the stack is empty. Instants, flash and abilities are what you respond with.',
    setup: () => createState(),
    steps: [
      {
        say: 'It is your turn, the stack is empty: you cast Divination. Allowed.',
        act: cast('you', 'divination'),
      },
      {
        say: 'You try to cast Wrath of God while Divination is on the stack. Refused: the stack isn’t empty.',
        act: cast('you', 'wrath-of-god'),
      },
      { say: 'You pass.', act: pass('you') },
      {
        say: 'The opponent tries to cast Divination in response. Refused — it isn’t their turn (the log names the first rule it hits; the non-empty stack would stop it too).',
        act: cast('opp', 'divination'),
      },
      {
        say: 'The opponent casts Ambush Viper instead — a creature, but with flash, so it can be cast whenever an instant could.',
        act: cast('opp', 'ambush-viper'),
      },
      { say: 'The opponent passes.', act: pass('opp') },
      { say: 'You pass: the Viper resolves and enters the battlefield.', act: pass('you') },
      { say: 'You pass.', act: pass('you') },
      { say: 'The opponent passes: Divination resolves.', act: pass('opp') },
    ],
  },
  {
    slug: 'stifle-a-trigger',
    title: 'Countering an ability',
    lesson:
      'An ability on the stack is separate from its source. Countering the ability removes only the ability — the creature that triggered it is already on the battlefield and stays there.',
    setup: () => createState(),
    steps: [
      { say: 'You cast Elvish Visionary.', act: cast('you', 'elvish-visionary') },
      { say: 'You pass.', act: pass('you') },
      {
        say: 'The opponent passes. The Visionary resolves and enters; its "when this enters" ability triggers and goes on the stack.',
        act: pass('opp'),
      },
      { say: 'You pass.', act: pass('you') },
      {
        say: 'The opponent casts Stifle targeting the trigger. Counterspell couldn’t do this — it counters spells, and a trigger is an ability.',
        act: cast('opp', 'stifle', top),
      },
      { say: 'The opponent passes.', act: pass('opp') },
      {
        say: 'You pass. Stifle resolves: the trigger is countered and ceases to exist. Elvish Visionary is still on the battlefield.',
        act: pass('you'),
      },
    ],
  },
  {
    slug: 'copy',
    title: 'Holding priority and copying a spell',
    lesson:
      'You keep priority after casting, so you can respond to your own spell before the opponent gets a chance. Fork puts a copy of the Bolt directly on the stack (it isn’t cast); the copy resolves before the original and then ceases to exist instead of going to a graveyard.',
    setup: () => createState(),
    steps: [
      {
        say: 'You cast Lightning Bolt at the opponent. Priority comes straight back to you — you have not passed yet.',
        act: cast('you', 'lightning-bolt', () => player('opp')),
      },
      {
        say: 'Holding priority, you cast Fork targeting your own Bolt. The opponent has had no chance to respond to the Bolt.',
        act: cast('you', 'fork', (s) => spell(s, 'lightning-bolt')),
      },
      { say: 'Now you pass. The opponent sees two spells on the stack at once.', act: pass('you') },
      {
        say: 'The opponent passes. Fork resolves and puts a copy of Lightning Bolt on the stack, above the original — with the same target.',
        act: pass('opp'),
      },
      { say: 'You pass.', act: pass('you') },
      { say: 'The opponent passes: the copy resolves and ceases to exist.', act: pass('opp') },
      { say: 'You pass.', act: pass('you') },
      {
        say: 'The opponent passes: the original Bolt resolves and goes to the graveyard.',
        act: pass('opp'),
      },
    ],
  },
]

export function scenarioBySlug(slug: string): Scenario | undefined {
  return SCENARIOS.find((s) => s.slug === slug)
}
