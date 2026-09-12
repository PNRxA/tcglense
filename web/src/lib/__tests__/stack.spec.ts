import { describe, expect, it } from 'vitest'
import { activatability, applyAction, castability, lastRefusal } from '@/lib/stack/engine'
import { nextHint } from '@/lib/stack/hints'
import { CARDS, cardById } from '@/lib/stack/cards'
import { RULES } from '@/lib/stack/rules'
import { SCENARIOS } from '@/lib/stack/scenarios'
import { createState } from '@/lib/stack/state'
import { legalTargets, preferredTargets } from '@/lib/stack/targets'
import type { Action, PlayerId, StackState, TargetRef } from '@/lib/stack/types'

function run(state: StackState, ...actions: Action[]): StackState {
  return actions.reduce((current, action) => applyAction(current, action), state)
}

const pass = (player: PlayerId): Action => ({ type: 'pass', player })
const cast = (player: PlayerId, cardId: string, target?: TargetRef): Action => ({
  type: 'cast',
  player,
  cardId,
  ...(target ? { target } : {}),
})
const put = (player: PlayerId, cardId: string): Action => ({
  type: 'put-onto-battlefield',
  player,
  cardId,
})

function permanentRef(state: StackState, cardId: string, controller?: PlayerId): TargetRef {
  const found = state.battlefield.find(
    (p) => p.cardId === cardId && (!controller || p.controller === controller),
  )
  if (!found) throw new Error(`${cardId} not on the battlefield`)
  return { kind: 'permanent', id: found.id }
}

function topRef(state: StackState): TargetRef {
  const top = state.stack[state.stack.length - 1]
  if (!top) throw new Error('empty stack')
  return { kind: 'stack', id: top.id }
}

const names = (state: StackState) => state.stack.map((o) => o.name)
const rulesCited = (state: StackState) => state.log.map((e) => e.rule).filter(Boolean)

describe('stack engine: casting and priority', () => {
  it('puts a cast spell on the stack and hands priority back to the caster', () => {
    const state = run(createState(), cast('you', 'lightning-bolt', { kind: 'player', id: 'opp' }))
    expect(names(state)).toEqual(['Lightning Bolt'])
    expect(state.priority).toBe('you')
    expect(state.passes).toEqual([])
    expect(rulesCited(state)).toContain('117.3c')
    expect(lastRefusal(state)).toBeNull()
  })

  it('refuses an action from the player without priority and changes nothing else', () => {
    const before = createState()
    const state = run(before, cast('opp', 'lightning-bolt', { kind: 'player', id: 'you' }))
    expect(state.stack).toEqual([])
    expect(lastRefusal(state)).toMatch(/doesn't have priority/)
    expect(state.log[state.log.length - 1]?.rule).toBe('117.1')
    expect(castability(before, 'opp', cardById('lightning-bolt')!)).toMatchObject({
      ok: false,
      rule: '117.1',
    })
  })

  it('never mutates the state it was handed', () => {
    const before = createState()
    const snapshot = JSON.stringify(before)
    run(
      before,
      cast('you', 'lightning-bolt', { kind: 'player', id: 'opp' }),
      pass('you'),
      pass('opp'),
    )
    expect(JSON.stringify(before)).toBe(snapshot)
  })

  it('resolves the top only once both players pass in succession, then gives the active player priority', () => {
    let state = run(
      createState(),
      cast('you', 'lightning-bolt', { kind: 'player', id: 'opp' }),
      pass('you'),
    )
    expect(state.priority).toBe('opp')
    expect(state.passes).toEqual(['you'])
    expect(names(state)).toEqual(['Lightning Bolt'])
    state = run(state, pass('opp'))
    expect(names(state)).toEqual([])
    expect(state.players.opp.life).toBe(17)
    expect(state.priority).toBe('you')
    expect(state.graveyard).toEqual([{ name: 'Lightning Bolt', owner: 'you' }])
    expect(rulesCited(state)).toEqual(expect.arrayContaining(['117.4', '117.3b', '608.2']))
  })

  it('resets the passing count when anyone acts in between', () => {
    let state = run(
      createState({ activePlayer: 'opp' }),
      cast('opp', 'lightning-bolt', { kind: 'player', id: 'you' }),
      pass('opp'),
    )
    state = run(state, cast('you', 'counterspell', topRef(state)))
    expect(state.passes).toEqual([])
    expect(state.priority).toBe('you')
    state = run(state, pass('you'))
    // The opponent's earlier pass no longer counts: nothing resolved on your single pass.
    expect(names(state)).toEqual(['Lightning Bolt', 'Counterspell'])
    expect(state.priority).toBe('opp')
  })

  it('ends the phase and moves the turn on when both pass on an empty stack', () => {
    let state = run(createState(), put('you', 'grizzly-bears'))
    state = run(
      state,
      cast('you', 'giant-growth', permanentRef(state, 'grizzly-bears')),
      pass('you'),
      pass('opp'),
    )
    expect(state.battlefield[0]?.pumpPower).toBe(3)
    state = run(state, pass('you'), pass('opp'))
    expect(state.turn).toBe(2)
    expect(state.activePlayer).toBe('opp')
    expect(state.priority).toBe('opp')
    expect(state.battlefield[0]?.pumpPower).toBe(0)
    expect(rulesCited(state)).toEqual(expect.arrayContaining(['500.2', '514.2']))
  })
})

describe('stack engine: last in, first out', () => {
  it('lets a counterspell in response resolve first and counter the bolt', () => {
    let state = run(
      createState(),
      cast('you', 'lightning-bolt', { kind: 'player', id: 'opp' }),
      pass('you'),
    )
    state = run(state, cast('opp', 'counterspell', topRef(state)))
    expect(names(state)).toEqual(['Lightning Bolt', 'Counterspell'])
    state = run(state, pass('opp'), pass('you'))
    expect(names(state)).toEqual([])
    expect(state.players.opp.life).toBe(20)
    expect(state.graveyard.map((g) => g.name)).toEqual(['Lightning Bolt', 'Counterspell'])
    expect(rulesCited(state)).toContain('701.5')
  })

  it('counters the counterspell and lets the original resolve afterwards', () => {
    let state = run(
      createState(),
      cast('you', 'lightning-bolt', { kind: 'player', id: 'opp' }),
      pass('you'),
    )
    state = run(state, cast('opp', 'counterspell', topRef(state)), pass('opp'))
    state = run(state, cast('you', 'negate', topRef(state)))
    expect(names(state)).toEqual(['Lightning Bolt', 'Counterspell', 'Negate'])
    state = run(state, pass('you'), pass('opp'))
    expect(names(state)).toEqual(['Lightning Bolt'])
    state = run(state, pass('you'), pass('opp'))
    expect(names(state)).toEqual([])
    expect(state.players.opp.life).toBe(17)
  })

  it('refuses Negate on a creature spell but allows Counterspell', () => {
    const state = run(createState(), cast('you', 'grizzly-bears'), pass('you'))
    expect(legalTargets(state, { kind: 'noncreature-spell' })).toEqual([])
    expect(legalTargets(state, { kind: 'spell' })).toEqual([topRef(state)])
    const refused = run(state, cast('opp', 'negate', topRef(state)))
    expect(lastRefusal(refused)).toMatch(/needs a noncreature spell/)
  })
})

describe('stack engine: timing restrictions', () => {
  it('refuses a sorcery while the stack is not empty, and on the other player’s turn', () => {
    const state = run(createState(), cast('you', 'lightning-bolt', { kind: 'player', id: 'opp' }))
    expect(lastRefusal(run(state, cast('you', 'divination')))).toMatch(/stack is empty/)
    const theirTurn = run(createState({ activePlayer: 'opp' }), pass('opp'))
    expect(theirTurn.priority).toBe('you')
    expect(lastRefusal(run(theirTurn, cast('you', 'divination')))).toMatch(/own main phase/)
    expect(lastRefusal(run(theirTurn, cast('you', 'grizzly-bears')))).toMatch(
      /creature spell without flash/,
    )
  })

  it('allows a flash creature whenever an instant could be cast', () => {
    const state = run(
      createState(),
      cast('you', 'lightning-bolt', { kind: 'player', id: 'opp' }),
      pass('you'),
    )
    const next = run(state, cast('opp', 'ambush-viper'))
    expect(lastRefusal(next)).toBeNull()
    expect(names(next)).toEqual(['Lightning Bolt', 'Ambush Viper'])
    expect(rulesCited(next)).toContain('702.8')
  })

  it('refuses a targeted spell with nothing to target', () => {
    const state = run(createState(), cast('you', 'counterspell'))
    expect(lastRefusal(state)).toMatch(/needs a spell on the stack, and there is none/)
  })
})

describe('stack engine: split second and mana abilities', () => {
  it('blocks spells and non-mana abilities but not mana abilities or triggers', () => {
    let state = run(
      createState(),
      put('opp', 'sol-ring'),
      put('opp', 'llanowar-elves'),
      put('opp', 'prodigal-sorcerer'),
      put('you', 'guttersnipe'),
    )
    state = run(state, cast('you', 'krosan-grip', permanentRef(state, 'sol-ring')))
    // The Guttersnipe trigger still went on the stack, above the Grip.
    expect(names(state)).toEqual(['Krosan Grip', "Guttersnipe's trigger"])
    state = run(state, pass('you'))
    const counter = run(
      state,
      cast('opp', 'counterspell', { kind: 'stack', id: state.stack[0]!.id }),
    )
    expect(lastRefusal(counter)).toMatch(/split second/)
    expect(counter.log[counter.log.length - 1]?.rule).toBe('702.61')
    const elves = state.battlefield.find((p) => p.cardId === 'llanowar-elves')!
    const tim = state.battlefield.find((p) => p.cardId === 'prodigal-sorcerer')!
    expect(activatability(state, 'opp', tim)).toMatchObject({ ok: false, rule: '702.61' })
    expect(activatability(state, 'opp', elves)).toEqual({ ok: true })
    const tapped = run(state, { type: 'activate', player: 'opp', permanentId: elves.id })
    expect(lastRefusal(tapped)).toBeNull()
    expect(names(tapped)).toEqual(['Krosan Grip', "Guttersnipe's trigger"])
    expect(tapped.battlefield.find((p) => p.id === elves.id)?.tapped).toBe(true)
    expect(tapped.log[tapped.log.length - 1]?.rule).toBe('605.3')
    // Tapping for mana is still an action: the passing count restarted.
    expect(tapped.passes).toEqual([])
  })

  it('refuses a tapped ability until the controller’s next untap', () => {
    let state = run(createState(), put('you', 'prodigal-sorcerer'))
    const tim = state.battlefield[0]!
    state = run(state, {
      type: 'activate',
      player: 'you',
      permanentId: tim.id,
      target: { kind: 'player', id: 'opp' },
    })
    expect(names(state)).toEqual(["Prodigal Sorcerer's ability"])
    state = run(state, pass('you'), pass('opp'))
    expect(state.players.opp.life).toBe(19)
    const again = run(state, {
      type: 'activate',
      player: 'you',
      permanentId: tim.id,
      target: { kind: 'player', id: 'opp' },
    })
    expect(lastRefusal(again)).toMatch(/already tapped/)
    // Two turns on (back to your turn) it has untapped.
    const later = run(state, pass('you'), pass('opp'), pass('opp'), pass('you'))
    expect(later.activePlayer).toBe('you')
    expect(later.battlefield[0]?.tapped).toBe(false)
  })
})

describe('stack engine: resolution, targets and state-based actions', () => {
  it('fizzles a pump spell whose creature died to the response', () => {
    let state = run(createState(), put('you', 'grizzly-bears'))
    const bears = permanentRef(state, 'grizzly-bears')
    state = run(
      state,
      cast('you', 'giant-growth', bears),
      pass('you'),
      cast('opp', 'lightning-bolt', bears),
      pass('opp'),
      pass('you'),
    )
    expect(state.battlefield).toEqual([])
    expect(rulesCited(state)).toContain('704.5g')
    expect(names(state)).toEqual(['Giant Growth'])
    state = run(state, pass('you'), pass('opp'))
    expect(names(state)).toEqual([])
    expect(rulesCited(state)).toContain('608.2b')
    expect(state.log.some((e) => /fizzles/.test(e.text))).toBe(true)
  })

  it('saves the creature when the pump resolves before the burn', () => {
    let state = run(createState({ activePlayer: 'opp' }), put('you', 'grizzly-bears'))
    const bears = permanentRef(state, 'grizzly-bears')
    state = run(
      state,
      cast('opp', 'lightning-bolt', bears),
      pass('opp'),
      cast('you', 'giant-growth', bears),
      pass('you'),
      pass('opp'),
    )
    expect(state.battlefield[0]?.pumpToughness).toBe(3)
    state = run(state, pass('opp'), pass('you'))
    expect(state.battlefield).toHaveLength(1)
    expect(state.battlefield[0]?.damage).toBe(3)
  })

  it('ends the game when a player reaches 0 life and refuses further actions', () => {
    let state = createState({ life: 3 })
    state = run(
      state,
      cast('you', 'lightning-bolt', { kind: 'player', id: 'opp' }),
      pass('you'),
      pass('opp'),
    )
    expect(state.loser).toBe('opp')
    expect(rulesCited(state)).toContain('704.5a')
    expect(nextHint(state).text).toMatch(/game is over/)
    expect(lastRefusal(run(state, cast('you', 'shock', { kind: 'player', id: 'opp' })))).toMatch(
      /game is over/,
    )
  })

  it('puts a resolved creature onto the battlefield and stacks its enters trigger', () => {
    const state = run(createState(), cast('you', 'elvish-visionary'), pass('you'), pass('opp'))
    expect(state.battlefield.map((p) => p.name)).toEqual(['Elvish Visionary'])
    expect(names(state)).toEqual(["Elvish Visionary's trigger"])
    expect(rulesCited(state)).toEqual(expect.arrayContaining(['608.3', '603.2', '603.3']))
  })

  it('lets Stifle counter a trigger without touching its source', () => {
    let state = run(
      createState(),
      cast('you', 'elvish-visionary'),
      pass('you'),
      pass('opp'),
      pass('you'),
    )
    state = run(state, cast('opp', 'stifle', topRef(state)), pass('opp'), pass('you'))
    expect(names(state)).toEqual([])
    expect(state.battlefield.map((p) => p.name)).toEqual(['Elvish Visionary'])
    expect(state.log.some((e) => /ceases to exist/.test(e.text))).toBe(true)
  })

  it('puts a cast trigger above the spell so it resolves first', () => {
    let state = run(createState(), put('you', 'guttersnipe'))
    state = run(state, cast('you', 'shock', { kind: 'player', id: 'opp' }))
    expect(names(state)).toEqual(['Shock', "Guttersnipe's trigger"])
    state = run(state, pass('you'), pass('opp'))
    expect(state.players.opp.life).toBe(18)
    expect(names(state)).toEqual(['Shock'])
  })

  it('orders simultaneous triggers APNAP so the non-active player’s resolves first', () => {
    let state = run(createState(), put('you', 'soul-warden'), put('opp', 'soul-warden'))
    state = run(state, cast('you', 'grizzly-bears'), pass('you'), pass('opp'))
    expect(state.stack.map((o) => o.controller)).toEqual(['you', 'opp'])
    expect(rulesCited(state)).toContain('603.3b')
    state = run(state, pass('you'), pass('opp'))
    expect(state.players.opp.life).toBe(21)
    expect(state.players.you.life).toBe(20)
  })

  it('copies a spell onto the stack above the original and the copy ceases to exist', () => {
    let state = run(
      createState({ activePlayer: 'opp' }),
      cast('opp', 'lightning-bolt', { kind: 'player', id: 'you' }),
      pass('opp'),
    )
    state = run(state, cast('you', 'fork', topRef(state)), pass('you'), pass('opp'))
    expect(names(state)).toEqual(['Lightning Bolt', 'Copy of Lightning Bolt'])
    expect(state.stack[1]?.kind).toBe('copy')
    expect(legalTargets(state, { kind: 'spell' })).toHaveLength(2)
    state = run(state, pass('opp'), pass('you'))
    expect(state.players.you.life).toBe(17)
    expect(state.graveyard.map((g) => g.name)).toEqual(['Fork'])
    expect(rulesCited(state)).toContain('707.10')
  })

  it('triggers a dies ability once per creature that died in the same event', () => {
    let state = run(
      createState(),
      put('you', 'zulaport-cutthroat'),
      put('you', 'grizzly-bears'),
      put('opp', 'grizzly-bears'),
    )
    state = run(state, cast('you', 'wrath-of-god'), pass('you'), pass('opp'))
    expect(state.battlefield).toEqual([])
    // The Cutthroat sees itself and your Bears die; the opponent's Bears is not "a creature you control".
    expect(names(state)).toEqual(["Zulaport Cutthroat's trigger", "Zulaport Cutthroat's trigger"])
  })

  it('prefers the opponent’s things for harm and your own for a pump', () => {
    const state = run(createState(), put('you', 'grizzly-bears'), put('opp', 'grizzly-bears'))
    const harm = preferredTargets(state, { kind: 'any' }, 'you', { kind: 'damage', amount: 3 })
    expect(harm[0]).toEqual(permanentRef(state, 'grizzly-bears', 'opp'))
    const help = preferredTargets(state, { kind: 'creature' }, 'you', {
      kind: 'pump',
      power: 3,
      toughness: 3,
    })
    expect(help[0]).toEqual(permanentRef(state, 'grizzly-bears', 'you'))
  })
})

describe('stack engine: hints and citations', () => {
  it('describes an empty stack, a waiting spell and a half-completed pass', () => {
    const empty = createState()
    expect(nextHint(empty).text).toMatch(/stack is empty and you have priority/)
    const waiting = run(empty, cast('you', 'lightning-bolt', { kind: 'player', id: 'opp' }))
    expect(nextHint(waiting).text).toMatch(/Lightning Bolt is on top of the stack/)
    const half = run(waiting, pass('you'))
    expect(nextHint(half).text).toMatch(/You passed\. Opponent now has priority/)
  })

  it('only cites rules the table knows', () => {
    let state = run(createState(), put('you', 'guttersnipe'), put('opp', 'soul-warden'))
    state = run(
      state,
      cast('you', 'grizzly-bears'),
      pass('you'),
      pass('opp'),
      pass('you'),
      pass('opp'),
    )
    const unknown = state.log.filter((e) => e.rule && !RULES[e.rule]).map((e) => e.rule)
    expect(unknown).toEqual([])
    const hint = nextHint(state)
    expect(hint.rule && RULES[hint.rule]).toBeDefined()
  })

  it('has a unique id per card and an effect on every instant and sorcery', () => {
    const ids = new Set(CARDS.map((c) => c.id))
    expect(ids.size).toBe(CARDS.length)
    const effectless = CARDS.filter(
      (c) => (c.type === 'instant' || c.type === 'sorcery') && !c.effect,
    )
    expect(effectless.map((c) => c.name)).toEqual([])
    const statless = CARDS.filter(
      (c) => c.type === 'creature' && (c.power == null || c.toughness == null),
    )
    expect(statless.map((c) => c.name)).toEqual([])
  })
})

describe('guided scenarios', () => {
  it.each(SCENARIOS.map((s) => [s.slug, s] as const))(
    '%s plays through with the story it tells',
    (_slug, scenario) => {
      let state = scenario.setup()
      expect(state.log).toEqual([])
      // A step that narrates a refusal must be refused; every other step must go through.
      const refusals = scenario.steps.map((step) => {
        state = applyAction(state, step.act(state))
        return lastRefusal(state) !== null
      })
      expect(refusals).toEqual(scenario.steps.map((step) => /refused/i.test(step.say)))
      // Every walkthrough ends with the stack cleared: the lesson is what resolved and how.
      expect(state.stack).toEqual([])
    },
  )

  it('ends the scenarios on the state each lesson promises', () => {
    const play = (slug: string) => {
      const scenario = SCENARIOS.find((s) => s.slug === slug)!
      return scenario.steps.reduce(
        (state, step) => applyAction(state, step.act(state)),
        scenario.setup(),
      )
    }
    expect(play('last-in-first-out').players.opp.life).toBe(20)
    expect(play('counter-war').players.opp.life).toBe(17)
    expect(play('fizzle').battlefield).toEqual([])
    expect(play('save-the-bear').battlefield.map((p) => p.name)).toEqual(['Grizzly Bears'])
    expect(play('cast-trigger').players.opp.life).toBe(16)
    expect(play('apnap').players.you.life).toBe(21)
    expect(play('split-second').battlefield.map((p) => p.name)).not.toContain('Sol Ring')
    expect(play('sorcery-timing').battlefield.map((p) => p.name)).toEqual(['Ambush Viper'])
    expect(play('stifle-a-trigger').battlefield.map((p) => p.name)).toEqual(['Elvish Visionary'])
    expect(play('copy').players.you.life).toBe(14)
  })
})
