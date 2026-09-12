import { describe, expect, it } from 'vitest'
import { useStackSimulator } from '@/composables/useStackSimulator'
import { SCENARIOS } from '@/lib/stack/scenarios'

// The session around the engine: history-as-undo, the walkthrough cursor, and the rule that a
// free action leaves the script. The engine's own rules are pinned in lib/__tests__/stack.spec.
describe('useStackSimulator', () => {
  it('starts on a clean table with nothing to undo', () => {
    const sim = useStackSimulator()
    expect(sim.state.value.stack).toEqual([])
    expect(sim.canUndo.value).toBe(false)
    expect(sim.canRedo.value).toBe(false)
    expect(sim.scenario.value).toBeNull()
  })

  it('undoes and redoes by walking the history', () => {
    const sim = useStackSimulator()
    sim.dispatch({
      type: 'cast',
      player: 'you',
      cardId: 'lightning-bolt',
      target: { kind: 'player', id: 'opp' },
    })
    expect(sim.state.value.stack).toHaveLength(1)
    expect(sim.latestEntries.value.length).toBeGreaterThan(0)
    sim.undo()
    expect(sim.state.value.stack).toEqual([])
    expect(sim.canRedo.value).toBe(true)
    sim.redo()
    expect(sim.state.value.stack).toHaveLength(1)
    // A new action after an undo discards the redo branch.
    sim.undo()
    sim.dispatch({ type: 'pass', player: 'you' })
    expect(sim.canRedo.value).toBe(false)
  })

  it('steps a walkthrough, rewinds its cursor on undo, and finishes', () => {
    const sim = useStackSimulator()
    const scenario = SCENARIOS[0]!
    sim.loadScenario(scenario.slug)
    expect(sim.scenario.value?.slug).toBe(scenario.slug)
    expect(sim.currentStep.value?.say).toBe(scenario.steps[0]!.say)
    sim.nextStep()
    expect(sim.stepIndex.value).toBe(1)
    expect(sim.state.value.stack).toHaveLength(1)
    sim.undo()
    expect(sim.stepIndex.value).toBe(0)
    expect(sim.state.value.stack).toEqual([])
    sim.redo()
    expect(sim.stepIndex.value).toBe(1)
    for (let i = 1; i < scenario.steps.length; i++) sim.nextStep()
    expect(sim.walkthroughDone.value).toBe(true)
    expect(sim.currentStep.value).toBeNull()
    // Stepping past the end is a no-op.
    sim.nextStep()
    expect(sim.stepIndex.value).toBe(scenario.steps.length)
  })

  it('leaves the walkthrough on a free action and resets to its setup until then', () => {
    const sim = useStackSimulator()
    sim.loadScenario('fizzle')
    expect(sim.state.value.battlefield.map((p) => p.name)).toEqual(['Grizzly Bears'])
    sim.nextStep()
    sim.reset()
    expect(sim.stepIndex.value).toBe(0)
    expect(sim.state.value.stack).toEqual([])
    expect(sim.state.value.battlefield).toHaveLength(1)
    sim.dispatch({ type: 'pass', player: 'you' })
    expect(sim.scenario.value).toBeNull()
    sim.loadScenario(null)
    expect(sim.state.value.battlefield).toEqual([])
  })

  it('surfaces the last refusal and the hint for the current table', () => {
    const sim = useStackSimulator()
    sim.dispatch({
      type: 'cast',
      player: 'opp',
      cardId: 'lightning-bolt',
      target: { kind: 'player', id: 'you' },
    })
    expect(sim.refusal.value).toMatch(/doesn't have priority/)
    expect(sim.hint.value.text).toMatch(/stack is empty/)
  })
})
